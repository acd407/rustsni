//! A poll-friendly StatusNotifierItem host library built on rustbus.
#![allow(clippy::mutable_key_type)]

mod host;
mod icon;
mod item;
mod menu;
mod watcher;

use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsRawFd, RawFd};

use rustbus::connection::Timeout;
use rustbus::connection::ll_conn::DuplexConn;

pub use icon::{IconPixmap, from_tuples};
pub use item::{ItemId, ToolTip, TrayItem};
pub use menu::{MenuItemProps, MenuNode, PropValue, fire_click as fire_menu_click};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("D-Bus error: {0}")]
    Bus(#[from] rustbus::connection::Error),
    #[error("marshal error: {0}")]
    Marshal(#[from] rustbus::wire::errors::MarshalError),
    #[error("unmarshal error: {0}")]
    Unmarshal(#[from] rustbus::wire::errors::UnmarshalError),
    #[error("another StatusNotifierWatcher is already running")]
    WatcherAlreadyRunning,
    #[error("item not found: {0}")]
    ItemNotFound(ItemId),
    #[error("D-Bus method call failed: {0}")]
    MethodCall(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    ItemAdded(ItemId),
    ItemChanged(ItemId),
    ItemRemoved(ItemId),
    MenuChanged(ItemId),
    MenuActivationRequested(ItemId),
    HostShutdown,
}

pub struct TrayHost {
    conn: DuplexConn,
    items: HashMap<ItemId, TrayItem>,
    pending_events: Vec<TrayEvent>,
    /// Unique names (:1.xxx) not yet probed for SNI support.
    pending_unique_names: Vec<String>,
    /// Retry count per unique name (discard after 3 consecutive timeouts).
    pending_unique_retries: HashMap<String, u32>,
    /// Serial of an in-flight GetAll probe, if any.
    probe_serial: Option<(u32, String, std::time::Instant)>,
    /// Messages buffered during synchronous property reads.
    pending_messages: VecDeque<rustbus::message_builder::MarshalledMessage>,
}

impl TrayHost {
    /// Connect to the session bus, register as StatusNotifierWatcher and
    /// StatusNotifierHost. Returns `Err(WatcherAlreadyRunning)` if another
    /// watcher is already active.
    pub fn new() -> Result<Self> {
        let mut conn = DuplexConn::connect_to_bus(rustbus::get_session_bus_path()?, true)?;
        conn.send_hello(Timeout::Infinite)?;

        let mut this = Self {
            conn,
            items: HashMap::new(),
            pending_events: Vec::new(),
            pending_unique_names: Vec::new(),
            pending_unique_retries: HashMap::new(),
            probe_serial: None,
            pending_messages: VecDeque::new(),
        };

        watcher::register(&mut this.conn)?;
        host::register(&mut this.conn)?;

        // Discover already-running tray items (bar-started-after-apps case)
        match watcher::discover_existing_items(&mut this.conn, &mut this.pending_messages) {
            Ok(pending) => {
                this.pending_unique_names = pending;
            }
            Err(_e) => {}
        }

        Ok(this)
    }

    /// The D-Bus file descriptor. Register this with your poll/epoll loop.
    pub fn fd(&self) -> RawFd {
        self.conn.as_raw_fd()
    }

    /// Non-blocking: read all pending D-Bus messages and return tray events.
    /// Call this when `fd()` becomes readable.
    ///
    /// Also probes one pending unique name per call (async, non-blocking).
    /// The probe never blocks the caller — it sends GetAll, stores the serial,
    /// and processes the reply on the next `poll()` call.
    pub fn poll(&mut self) -> Result<Vec<TrayEvent>> {
        use rustbus::message_builder::MessageType;

        let mut events = self.pending_events.drain(..).collect();

        // Process any messages buffered during property reads
        while let Some(msg) = self.pending_messages.pop_front() {
            self.dispatch(msg, &mut events)?;
        }

        // Check if the active probe's reply has arrived
        let mut probe_done = false;

        // Read available messages from the socket
        loop {
            match self.conn.recv.read_once(Timeout::Nonblock) {
                Ok(()) => {}
                Err(rustbus::connection::Error::TimedOut) => break,
                Err(e) => return Err(e.into()),
            }
            if !self.conn.recv.buffer_contains_whole_message()? {
                break;
            }
            let msg = self.conn.recv.get_next_message(Timeout::Nonblock)?;

            // Check if this message matches the active probe
            if let Some((serial, _, _)) = self.probe_serial
                && msg.dynheader.response_serial == Some(serial)
            {
                match msg.typ {
                    MessageType::Reply => {
                        // Success — it's an SNI item
                        probe_done = true;
                        let name = self.probe_serial.take().unwrap().1;
                        self.process_probe_ok(&name, msg, &mut events);
                        continue;
                    }
                    MessageType::Error => {
                        // Error (UnknownInterface) — not an SNI item
                        probe_done = true;
                        self.probe_serial.take();
                        continue;
                    }
                    _ => {}
                }
            }

            // Not a probe reply — dispatch normally
            self.dispatch(msg, &mut events)?;
        }

        // Check probe timeout (500ms max per probe).
        // On timeout the name is re-queued (up to 3 retries) so it gets
        // retried on a later poll. After 3 consecutive timeouts the name is
        // discarded — it likely belongs to a non-SNI process that doesn't
        // respond to GetAll.
        if !probe_done
            && let Some((_, _, deadline)) = self.probe_serial.as_ref()
            && *deadline <= std::time::Instant::now()
        {
            let name = self.probe_serial.take().unwrap().1;
            let retries = self.pending_unique_retries.entry(name.clone()).or_insert(0);
            if *retries < 3 {
                *retries += 1;
                self.pending_unique_names.push(name);
            } else {
                self.pending_unique_retries.remove(&name);
            }
        }

        // Start next probe if none active and there are pending names
        if self.probe_serial.is_none() && !self.pending_unique_names.is_empty() {
            let name = self.pending_unique_names.remove(0);
            match item::TrayItem::send_get_all(&mut self.conn, &name, "/StatusNotifierItem") {
                Ok(serial) => {
                    let deadline =
                        std::time::Instant::now() + std::time::Duration::from_millis(500);
                    self.probe_serial = Some((serial, name, deadline));
                }
                Err(_) => {
                    // Failed to send — skip this name
                }
            }
        }

        Ok(events)
    }

    /// Process a successful probe reply (GetAll returned a valid dict).
    fn process_probe_ok(
        &mut self,
        name: &str,
        reply: rustbus::message_builder::MarshalledMessage,
        events: &mut Vec<TrayEvent>,
    ) {
        if let Ok(item) =
            item::TrayItem::from_get_all_reply(&reply, name, name, "/StatusNotifierItem")
        {
            // Deduplicate: skip if this item's `Id` already registered
            if self
                .items
                .values()
                .any(|e| !e.item_id.is_empty() && e.item_id == item.item_id)
            {
                return;
            }

            let id = item.id.clone();
            self.items.insert(id.clone(), item);

            let mut sig = rustbus::MessageBuilder::new()
                .signal(
                    crate::watcher::WATCHER_INTERFACE,
                    "StatusNotifierItemRegistered",
                    crate::watcher::WATCHER_PATH,
                )
                .build();
            sig.body.push_param(name).unwrap();
            let _ = self.conn.send.send_message_write_all(&sig);

            events.push(TrayEvent::ItemAdded(id));
        }
    }

    /// Current tray items.
    pub fn items(&self) -> &HashMap<ItemId, TrayItem> {
        &self.items
    }

    /// Call the item's `Activate` method.
    pub fn activate(&mut self, id: &ItemId, x: i32, y: i32) -> Result<()> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| crate::Error::ItemNotFound(id.clone()))?;
        item::call_method(
            &mut self.conn,
            &item.bus_name,
            &item.object_path,
            "Activate",
            x,
            y,
        )
    }

    /// Call the item's `ContextMenu` method.
    pub fn context_menu(&mut self, id: &ItemId, x: i32, y: i32) -> Result<()> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| crate::Error::ItemNotFound(id.clone()))?;
        item::call_method(
            &mut self.conn,
            &item.bus_name,
            &item.object_path,
            "ContextMenu",
            x,
            y,
        )
    }

    /// Call the item's `SecondaryActivate` method.
    pub fn secondary_activate(&mut self, id: &ItemId, x: i32, y: i32) -> Result<()> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| crate::Error::ItemNotFound(id.clone()))?;
        item::call_method(
            &mut self.conn,
            &item.bus_name,
            &item.object_path,
            "SecondaryActivate",
            x,
            y,
        )
    }

    /// Get the menu layout for a tray item.
    pub fn get_menu(&mut self, id: &ItemId, parent_id: i32) -> Result<Vec<menu::MenuNode>> {
        let item = match self.items.get(id) {
            Some(i) => i,
            None => return Ok(Vec::new()),
        };
        if item.menu_path.is_empty() || item.menu_path == "/" {
            return Ok(Vec::new());
        }
        menu::get_layout(&mut self.conn, &item.bus_name, &item.menu_path, parent_id)
    }

    /// Fire a click event on a menu item.
    pub fn menu_click(&mut self, id: &ItemId, menu_id: i32) -> Result<()> {
        let item = match self.items.get(id) {
            Some(i) => i,
            None => return Ok(()),
        };
        if item.menu_path.is_empty() || item.menu_path == "/" {
            return Ok(());
        }
        menu::event(
            &mut self.conn,
            &item.bus_name,
            &item.menu_path,
            menu_id,
            "clicked",
        )
    }

    /// Call the item's `Scroll` method.
    pub fn scroll(&mut self, id: &ItemId, delta: i32, orientation: &str) -> Result<()> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| crate::Error::ItemNotFound(id.clone()))?;
        item::call_method_i32_str(
            &mut self.conn,
            &item.bus_name,
            &item.object_path,
            "Scroll",
            delta,
            orientation,
        )
    }

    /// Call the item's `ProvideXdgActivationToken` method.
    pub fn provide_xdg_activation_token(&mut self, id: &ItemId, token: &str) -> Result<()> {
        let item = self
            .items
            .get(id)
            .ok_or_else(|| crate::Error::ItemNotFound(id.clone()))?;
        item::call_method_str(
            &mut self.conn,
            &item.bus_name,
            &item.object_path,
            "ProvideXdgActivationToken",
            token,
        )
    }

    /// Get properties for multiple menu items at once via `GetGroupProperties`.
    pub fn get_menu_group_properties(
        &mut self,
        id: &ItemId,
        ids: &[i32],
        property_names: &[&str],
    ) -> Result<Vec<menu::MenuItemProps>> {
        let item = match self.items.get(id) {
            Some(i) => i,
            None => return Ok(Vec::new()),
        };
        if item.menu_path.is_empty() || item.menu_path == "/" {
            return Ok(Vec::new());
        }
        menu::get_group_properties(
            &mut self.conn,
            &item.bus_name,
            &item.menu_path,
            ids,
            property_names,
        )
    }

    /// Get a single property from a menu item via `GetProperty`.
    pub fn get_menu_property(
        &mut self,
        id: &ItemId,
        menu_id: i32,
        name: &str,
    ) -> Result<Option<menu::PropValue>> {
        let item = match self.items.get(id) {
            Some(i) => i,
            None => return Ok(None),
        };
        if item.menu_path.is_empty() || item.menu_path == "/" {
            return Ok(None);
        }
        menu::get_property(
            &mut self.conn,
            &item.bus_name,
            &item.menu_path,
            menu_id,
            name,
        )
    }

    /// Emit `StatusNotifierHostUnregistered` and push `HostShutdown`.
    pub fn shutdown(&mut self) -> Result<()> {
        let sig = rustbus::MessageBuilder::new()
            .signal(
                "org.kde.StatusNotifierWatcher",
                "StatusNotifierHostUnregistered",
                "/StatusNotifierWatcher",
            )
            .build();
        self.conn.send.send_message_write_all(&sig)?;
        self.pending_events.push(TrayEvent::HostShutdown);
        Ok(())
    }

    fn dispatch(
        &mut self,
        msg: rustbus::message_builder::MarshalledMessage,
        events: &mut Vec<TrayEvent>,
    ) -> Result<()> {
        use rustbus::message_builder::MessageType;

        match msg.typ {
            MessageType::Signal => {
                watcher::handle_signal(
                    &mut self.conn,
                    &msg,
                    &mut self.items,
                    events,
                    &mut self.pending_messages,
                )?;
            }
            MessageType::Call => {
                // Handle all incoming method calls (watcher, properties, etc.)
                watcher::handle_call(
                    &mut self.conn,
                    &msg,
                    &mut self.items,
                    events,
                    &mut self.pending_messages,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}
