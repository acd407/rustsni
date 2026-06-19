//! A poll-friendly StatusNotifierItem host library built on rustbus.

mod host;
mod icon;
mod item;
pub mod menu;
mod watcher;

use std::collections::HashMap;
use std::os::fd::{AsRawFd, RawFd};

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::connection::Timeout;

pub use icon::{from_tuples, IconPixmap};
pub use item::{ItemId, ToolTip, TrayItem};

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
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    ItemAdded(ItemId),
    ItemChanged(ItemId),
    ItemRemoved(ItemId),
}

pub struct TrayHost {
    conn: DuplexConn,
    items: HashMap<ItemId, TrayItem>,
}

impl TrayHost {
    /// Connect to the session bus, register as StatusNotifierWatcher and
    /// StatusNotifierHost. Returns `Err(WatcherAlreadyRunning)` if another
    /// watcher is already active.
    pub fn new() -> Result<Self> {
        let mut conn = DuplexConn::connect_to_bus(
            rustbus::get_session_bus_path()?,
            true,
        )?;
        conn.send_hello(Timeout::Infinite)?;

        let mut this = Self {
            conn,
            items: HashMap::new(),
        };

        watcher::register(&mut this.conn)?;
        host::register(&mut this.conn)?;

        Ok(this)
    }

    /// The D-Bus file descriptor. Register this with your poll/epoll loop.
    pub fn fd(&self) -> RawFd {
        self.conn.as_raw_fd()
    }

    /// Non-blocking: read all pending D-Bus messages and return tray events.
    /// Call this when `fd()` becomes readable.
    pub fn poll(&mut self) -> Result<Vec<TrayEvent>> {
        let mut events = Vec::new();

        // First, process any messages buffered during property reads
        while let Some(msg) = item::take_pending_message() {
            self.dispatch(msg, &mut events)?;
        }

        // Then read from the socket
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
            self.dispatch(msg, &mut events)?;
        }
        Ok(events)
    }

    /// Current tray items.
    pub fn items(&self) -> &HashMap<ItemId, TrayItem> {
        &self.items
    }

    /// Call the item's `Activate` method.
    pub fn activate(&mut self, id: &ItemId, x: i32, y: i32) -> Result<()> {
        let item = self.items.get(id).ok_or_else(|| crate::Error::Unmarshal(
            rustbus::wire::errors::UnmarshalError::NotEnoughBytes,
        ))?;
        item::call_method(&mut self.conn, &item.bus_name, &item.object_path, "Activate", x, y)
    }

    /// Call the item's `ContextMenu` method.
    pub fn context_menu(&mut self, id: &ItemId, x: i32, y: i32) -> Result<()> {
        let item = self.items.get(id).ok_or_else(|| crate::Error::Unmarshal(
            rustbus::wire::errors::UnmarshalError::NotEnoughBytes,
        ))?;
        item::call_method(&mut self.conn, &item.bus_name, &item.object_path, "ContextMenu", x, y)
    }

    /// Call the item's `SecondaryActivate` method.
    pub fn secondary_activate(&mut self, id: &ItemId, x: i32, y: i32) -> Result<()> {
        let item = self.items.get(id).ok_or_else(|| crate::Error::Unmarshal(
            rustbus::wire::errors::UnmarshalError::NotEnoughBytes,
        ))?;
        item::call_method(&mut self.conn, &item.bus_name, &item.object_path, "SecondaryActivate", x, y)
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
        menu::event(&mut self.conn, &item.bus_name, &item.menu_path, menu_id, "clicked")
    }

    /// Call the item's `Scroll` method.
    pub fn scroll(&mut self, id: &ItemId, delta: i32, orientation: &str) -> Result<()> {
        let item = self.items.get(id).ok_or_else(|| crate::Error::Unmarshal(
            rustbus::wire::errors::UnmarshalError::NotEnoughBytes,
        ))?;
        let mut call = rustbus::MessageBuilder::new()
            .call("Scroll")
            .on(&item.object_path)
            .with_interface("org.kde.StatusNotifierItem")
            .at(&item.bus_name)
            .build();
        call.body.push_param(delta).unwrap();
        call.body.push_param(orientation).unwrap();
        self.conn.send.send_message_write_all(&call)?;
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
                watcher::handle_signal(&mut self.conn, &msg, &mut self.items, events)?;
            }
            MessageType::Call => {
                // Handle all incoming method calls (watcher, properties, etc.)
                watcher::handle_call(&mut self.conn, &msg, &mut self.items, events)?;
            }
            _ => {}
        }
        Ok(())
    }
}
