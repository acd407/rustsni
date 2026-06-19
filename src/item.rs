//! TrayItem data model and property reading.

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::connection::Timeout;

use crate::icon::IconPixmap;
use crate::Result;

/// A tray item's unique identifier — its D-Bus bus name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemId(pub String);

/// Resolved properties of a StatusNotifierItem.
#[derive(Debug, Clone)]
pub struct TrayItem {
    pub id: ItemId,
    pub bus_name: String,
    pub object_path: String,
    pub category: String,
    pub title: String,
    pub status: String,
    pub icon_name: String,
    pub icon_pixmaps: Vec<IconPixmap>,
    pub attention_icon_name: String,
    pub attention_icon_pixmaps: Vec<IconPixmap>,
    pub overlay_icon_name: String,
    pub overlay_icon_pixmaps: Vec<IconPixmap>,
    pub item_is_menu: bool,
    pub menu_path: String,
}

const SNI_IFACE: &str = "org.kde.StatusNotifierItem";

impl TrayItem {
    /// Read properties from the item over D-Bus using individual Get calls.
    pub fn from_bus(conn: &mut DuplexConn, bus_name: &str) -> Result<Self> {
        Self::from_bus_with_path(conn, bus_name, bus_name, "/StatusNotifierItem")
    }

    /// Read properties with a specific object path.
    pub fn from_bus_with_path(conn: &mut DuplexConn, service_id: &str, bus_name: &str, object_path: &str) -> Result<Self> {
        let mut item = TrayItem {
            id: ItemId(service_id.to_owned()),
            bus_name: bus_name.to_owned(),
            object_path: object_path.to_owned(),
            category: String::new(),
            title: String::new(),
            status: String::new(),
            icon_name: String::new(),
            icon_pixmaps: Vec::new(),
            attention_icon_name: String::new(),
            attention_icon_pixmaps: Vec::new(),
            overlay_icon_name: String::new(),
            overlay_icon_pixmaps: Vec::new(),
            item_is_menu: false,
            menu_path: String::new(),
        };

        item.category = get_string(conn, bus_name, object_path, "Category").unwrap_or_default();
        item.title = get_string(conn, bus_name, object_path, "Title").unwrap_or_default();
        item.status = get_string(conn, bus_name, object_path, "Status").unwrap_or_default();
        item.icon_name = get_string(conn, bus_name, object_path, "IconName").unwrap_or_default();
        item.attention_icon_name =
            get_string(conn, bus_name, object_path, "AttentionIconName").unwrap_or_default();
        item.overlay_icon_name =
            get_string(conn, bus_name, object_path, "OverlayIconName").unwrap_or_default();
        item.item_is_menu = get_bool(conn, bus_name, object_path, "ItemIsMenu").unwrap_or_default();
        item.menu_path = get_string(conn, bus_name, object_path, "Menu").unwrap_or_default();
        item.icon_pixmaps = get_pixmaps(conn, bus_name, object_path, "IconPixmap");
        item.attention_icon_pixmaps = get_pixmaps(conn, bus_name, object_path, "AttentionIconPixmap");
        item.overlay_icon_pixmaps = get_pixmaps(conn, bus_name, object_path, "OverlayIconPixmap");

        Ok(item)
    }
}

thread_local! {
    static PENDING_MESSAGES: std::cell::RefCell<Vec<rustbus::message_builder::MarshalledMessage>> =
        std::cell::RefCell::new(Vec::new());
}

/// Get a pending message that was received while waiting for a property response.
pub fn take_pending_message() -> Option<rustbus::message_builder::MarshalledMessage> {
    PENDING_MESSAGES.with(|msgs| msgs.borrow_mut().pop())
}

/// Call `Properties.Get(interface, property)`, skip signals, return reply.
/// Non-reply messages are buffered for later processing.
fn call_get_property(
    conn: &mut DuplexConn,
    bus_name: &str,
    object_path: &str,
    prop: &str,
) -> Result<rustbus::message_builder::MarshalledMessage> {
    let mut call = rustbus::MessageBuilder::new()
        .call("Get")
        .on(object_path)
        .with_interface("org.freedesktop.DBus.Properties")
        .at(bus_name)
        .build();
    call.body.push_param(SNI_IFACE).unwrap();
    call.body.push_param(prop).unwrap();

    let serial = conn.send.send_message_write_all(&call)?;
    loop {
        let resp = conn.recv.get_next_message(Timeout::Infinite)?;
        match resp.typ {
            rustbus::message_builder::MessageType::Reply => {
                if resp.dynheader.response_serial != Some(serial) {
                    return Err(crate::Error::Unmarshal(
                        rustbus::wire::errors::UnmarshalError::NotEnoughBytes,
                    ));
                }
                return Ok(resp);
            }
            rustbus::message_builder::MessageType::Error => {
                if resp.dynheader.response_serial == Some(serial) {
                    return Err(crate::Error::Unmarshal(
                        rustbus::wire::errors::UnmarshalError::NotEnoughBytes,
                    ));
                }
                // Error for a different message - buffer it
                PENDING_MESSAGES.with(|msgs| msgs.borrow_mut().push(resp));
            }
            _ => {
                // Signal or Call for a different message - buffer it
                PENDING_MESSAGES.with(|msgs| msgs.borrow_mut().push(resp));
            }
        }
    }
}

fn get_string(conn: &mut DuplexConn, bus_name: &str, object_path: &str, prop: &str) -> Result<String> {
    let resp = call_get_property(conn, bus_name, object_path, prop)?;
    let mut parser = resp.body.parser();
    let val: rustbus::wire::unmarshal::traits::Variant = parser.get()?;
    // Try string first, then object path
    if let Ok(s) = val.get::<&str>() {
        return Ok(s.to_owned());
    }
    if let Ok(p) = val.get::<rustbus::wire::ObjectPath<String>>() {
        return Ok(p.as_ref().to_owned());
    }
    Ok(String::new())
}

fn get_bool(conn: &mut DuplexConn, bus_name: &str, object_path: &str, prop: &str) -> Result<bool> {
    let resp = call_get_property(conn, bus_name, object_path, prop)?;
    let mut parser = resp.body.parser();
    let val: rustbus::wire::unmarshal::traits::Variant = parser.get()?;
    match val.get::<bool>() {
        Ok(b) => Ok(b),
        Err(_) => Ok(false),
    }
}

fn get_pixmaps(conn: &mut DuplexConn, bus_name: &str, object_path: &str, prop: &str) -> Vec<IconPixmap> {
    let resp = match call_get_property(conn, bus_name, object_path, prop) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut parser = resp.body.parser();
    let param = match parser.get_param() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    extract_pixmaps_from_param(&param)
}

/// Extract IconPixmaps from a Param value (variant containing a(iiay)).
fn extract_pixmaps_from_param(param: &rustbus::params::Param) -> Vec<IconPixmap> {
    use rustbus::params::{Container, Param};

    // The param is Variant(Array(Struct(i32, i32, Array(u8))))
    let inner = match param {
        Param::Container(Container::Variant(v)) => &v.value,
        _ => return Vec::new(),
    };

    let array = match inner {
        Param::Container(Container::Array(arr)) => &arr.values,
        _ => return Vec::new(),
    };

    let mut result = Vec::new();
    for elem in array {
        let fields = match elem {
            Param::Container(Container::Struct(s)) => s,
            _ => continue,
        };
        if fields.len() < 3 {
            continue;
        }
        let w = match &fields[0] {
            Param::Base(rustbus::params::Base::Int32(v)) => *v,
            _ => continue,
        };
        let h = match &fields[1] {
            Param::Base(rustbus::params::Base::Int32(v)) => *v,
            _ => continue,
        };
        let raw = match &fields[2] {
            Param::Container(Container::Array(arr)) => {
                arr.values.iter().filter_map(|b| {
                    if let Param::Base(rustbus::params::Base::Byte(v)) = b { Some(*v) } else { None }
                }).collect::<Vec<u8>>()
            }
            _ => continue,
        };

        if w <= 0 || h <= 0 {
            continue;
        }
        let expected = (w as usize) * (h as usize) * 4;
        if raw.len() < expected {
            continue;
        }

        let mut data = raw[..expected].to_vec();
        // big-endian [A,R,G,B] → native LE Cairo [B,G,R,A]
        for pixel in data.chunks_exact_mut(4) {
            let a = pixel[0];
            let r = pixel[1];
            let g = pixel[2];
            let b = pixel[3];
            pixel[0] = b;
            pixel[1] = g;
            pixel[2] = r;
            pixel[3] = a;
        }

        result.push(IconPixmap {
            width: w as u32,
            height: h as u32,
            data,
        });
    }
    result
}

/// Call a simple (x, y) method on a StatusNotifierItem.
pub fn call_method(
    conn: &mut DuplexConn,
    bus_name: &str,
    method: &str,
    x: i32,
    y: i32,
) -> Result<()> {
    let mut call = rustbus::MessageBuilder::new()
        .call(method)
        .on("/StatusNotifierItem")
        .with_interface(SNI_IFACE)
        .at(bus_name)
        .build();
    call.body.push_param(x).unwrap();
    call.body.push_param(y).unwrap();
    conn.send.send_message_write_all(&call)?;
    Ok(())
}
