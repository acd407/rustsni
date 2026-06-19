//! TrayItem data model and property reading.

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::connection::Timeout;

use crate::icon::IconPixmap;
use crate::Result;

/// A tray item's unique identifier — its D-Bus bus name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemId(pub String);

/// Tooltip data: icon name, optional icon pixmap, title, and descriptive text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolTip {
    pub icon_name: String,
    pub icon_pixmap: Option<IconPixmap>,
    pub title: String,
    pub text: String,
}

/// Resolved properties of a StatusNotifierItem.
#[derive(Debug, Clone)]
pub struct TrayItem {
    pub id: ItemId,
    pub bus_name: String,
    pub object_path: String,
    pub category: String,
    pub item_id: String,
    pub title: String,
    pub status: String,
    pub window_id: i32,
    pub icon_theme_path: String,
    pub icon_name: String,
    pub icon_pixmaps: Vec<IconPixmap>,
    pub attention_icon_name: String,
    pub attention_icon_pixmaps: Vec<IconPixmap>,
    pub attention_movie_name: String,
    pub overlay_icon_name: String,
    pub overlay_icon_pixmaps: Vec<IconPixmap>,
    pub item_is_menu: bool,
    pub menu_path: String,
    pub tooltip: ToolTip,
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
            item_id: String::new(),
            title: String::new(),
            status: String::new(),
            window_id: 0,
            icon_theme_path: String::new(),
            icon_name: String::new(),
            icon_pixmaps: Vec::new(),
            attention_icon_name: String::new(),
            attention_icon_pixmaps: Vec::new(),
            attention_movie_name: String::new(),
            overlay_icon_name: String::new(),
            overlay_icon_pixmaps: Vec::new(),
            item_is_menu: false,
            menu_path: String::new(),
            tooltip: ToolTip::default(),
        };

        item.category = get_string(conn, bus_name, object_path, "Category").unwrap_or_default();
        item.item_id = get_string(conn, bus_name, object_path, "Id").unwrap_or_default();
        item.title = get_string(conn, bus_name, object_path, "Title").unwrap_or_default();
        item.status = get_string(conn, bus_name, object_path, "Status").unwrap_or_default();
        item.window_id = get_int(conn, bus_name, object_path, "WindowId").unwrap_or(0);
        item.icon_theme_path = get_string(conn, bus_name, object_path, "IconThemePath").unwrap_or_default();
        item.icon_name = get_string(conn, bus_name, object_path, "IconName").unwrap_or_default();
        item.attention_icon_name =
            get_string(conn, bus_name, object_path, "AttentionIconName").unwrap_or_default();
        item.overlay_icon_name =
            get_string(conn, bus_name, object_path, "OverlayIconName").unwrap_or_default();
        item.attention_movie_name =
            get_string(conn, bus_name, object_path, "AttentionMovieName").unwrap_or_default();
        item.item_is_menu = get_bool(conn, bus_name, object_path, "ItemIsMenu").unwrap_or_default();
        item.menu_path = get_string(conn, bus_name, object_path, "Menu").unwrap_or_default();
        item.icon_pixmaps = get_pixmaps(conn, bus_name, object_path, "IconPixmap");
        item.attention_icon_pixmaps = get_pixmaps(conn, bus_name, object_path, "AttentionIconPixmap");
        item.overlay_icon_pixmaps = get_pixmaps(conn, bus_name, object_path, "OverlayIconPixmap");
        item.tooltip = get_tooltip(conn, bus_name, object_path);

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

fn get_int(conn: &mut DuplexConn, bus_name: &str, object_path: &str, prop: &str) -> Result<i32> {
    let resp = call_get_property(conn, bus_name, object_path, prop)?;
    let mut parser = resp.body.parser();
    let val: rustbus::wire::unmarshal::traits::Variant = parser.get()?;
    match val.get::<i32>() {
        Ok(v) => Ok(v),
        Err(_) => Ok(0),
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

fn get_tooltip(conn: &mut DuplexConn, bus_name: &str, object_path: &str) -> ToolTip {
    let resp = match call_get_property(conn, bus_name, object_path, "ToolTip") {
        Ok(r) => r,
        Err(_) => return ToolTip::default(),
    };
    let mut parser = resp.body.parser();
    let param = match parser.get_param() {
        Ok(p) => p,
        Err(_) => return ToolTip::default(),
    };
    extract_tooltip_from_param(&param)
}

/// Parse a variant containing (sa(iiay)ss) into a ToolTip.
fn extract_tooltip_from_param(param: &rustbus::params::Param) -> ToolTip {
    use rustbus::params::{Container, Param};

    let inner = match param {
        Param::Container(Container::Variant(v)) => &v.value,
        _ => return ToolTip::default(),
    };

    let fields = match inner {
        Param::Container(Container::Struct(s)) => s,
        _ => return ToolTip::default(),
    };
    if fields.len() < 4 {
        return ToolTip::default();
    }

    let icon_name = match &fields[0] {
        Param::Base(rustbus::params::Base::StringRef(s)) => s.to_string(),
        Param::Base(rustbus::params::Base::String(s)) => s.clone(),
        _ => String::new(),
    };

    // icon pixmap: a(iiay) — take the first image if present
    let icon_pixmap = match &fields[1] {
        Param::Container(Container::Array(arr)) => {
            arr.values.first().and_then(|elem| {
                let s = match elem {
                    Param::Container(Container::Struct(s)) => s,
                    _ => return None,
                };
                if s.len() < 3 {
                    return None;
                }
                let w = match &s[0] {
                    Param::Base(rustbus::params::Base::Int32(v)) => *v,
                    _ => return None,
                };
                let h = match &s[1] {
                    Param::Base(rustbus::params::Base::Int32(v)) => *v,
                    _ => return None,
                };
                let raw: Vec<u8> = match &s[2] {
                    Param::Container(Container::Array(a)) => a.values.iter().filter_map(|b| {
                        if let Param::Base(rustbus::params::Base::Byte(v)) = b { Some(*v) } else { None }
                    }).collect(),
                    _ => return None,
                };
                if w <= 0 || h <= 0 {
                    return None;
                }
                let expected = (w as usize) * (h as usize) * 4;
                if raw.len() < expected {
                    return None;
                }
                let mut data = raw[..expected].to_vec();
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
                Some(IconPixmap { width: w as u32, height: h as u32, data })
            })
        }
        _ => None,
    };

    let title = match &fields[2] {
        Param::Base(rustbus::params::Base::StringRef(s)) => s.to_string(),
        Param::Base(rustbus::params::Base::String(s)) => s.clone(),
        _ => String::new(),
    };

    let text = match &fields[3] {
        Param::Base(rustbus::params::Base::StringRef(s)) => s.to_string(),
        Param::Base(rustbus::params::Base::String(s)) => s.clone(),
        _ => String::new(),
    };

    ToolTip { icon_name, icon_pixmap, title, text }
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
    object_path: &str,
    method: &str,
    x: i32,
    y: i32,
) -> Result<()> {
    let mut call = rustbus::MessageBuilder::new()
        .call(method)
        .on(object_path)
        .with_interface(SNI_IFACE)
        .at(bus_name)
        .build();
    call.body.push_param(x).unwrap();
    call.body.push_param(y).unwrap();
    conn.send.send_message_write_all(&call)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustbus::params::{Base as PBase, Container, Param};

    /// Helper: create a signature::Type for a(iiay) — array of (i32,i32,ay).
    fn sig_icon_array() -> rustbus::signature::Type {
        use rustbus::signature::{Base as SBase, Container as SContainer, StructTypes, Type};
        Type::Container(SContainer::Array(Box::new(Type::Container(SContainer::Struct(
            StructTypes::new(vec![
                Type::Base(SBase::Int32),
                Type::Base(SBase::Int32),
                Type::Container(SContainer::Array(Box::new(Type::Base(SBase::Byte)))),
            ]).unwrap(),
        )))))
    }

    /// Build a Param tree for `(sa(iiay)ss)` and parse it as a ToolTip.
    #[test]
    fn tooltip_parse_full() {
        // (sa(iiay)ss) — struct with 4 fields
        let icon_name: Param = Param::Base(PBase::StringRef("my-icon"));
        let pixel_data: Param = Param::Container(Container::Array(rustbus::params::Array {
            element_sig: rustbus::signature::Type::Base(rustbus::signature::Base::Byte),
            values: vec![
                Param::Base(PBase::Byte(0xFF)), // A
                Param::Base(PBase::Byte(0x11)), // R
                Param::Base(PBase::Byte(0x22)), // G
                Param::Base(PBase::Byte(0x33)), // B
            ],
        }));
        let pixmap_struct: Param = Param::Container(Container::Struct(vec![
            Param::Base(PBase::Int32(1)),
            Param::Base(PBase::Int32(1)),
            pixel_data,
        ]));
        let pixmaps: Param = Param::Container(Container::Array(rustbus::params::Array {
            element_sig: sig_icon_array(),
            values: vec![pixmap_struct],
        }));
        let title: Param = Param::Base(PBase::StringRef("Title Text"));
        let text: Param = Param::Base(PBase::StringRef("Description"));

        let tooltip_struct: Param = Param::Container(Container::Struct(vec![
            icon_name,
            pixmaps,
            title,
            text,
        ]));

        // Wrap in variant (as Properties.Get returns)
        let variant: Param = Param::Container(Container::Variant(Box::new(rustbus::params::Variant {
            sig: rustbus::signature::Type::Base(rustbus::signature::Base::String),
            value: tooltip_struct,
        })));

        let tooltip = extract_tooltip_from_param(&variant);
        assert_eq!(tooltip.icon_name, "my-icon");
        assert_eq!(tooltip.title, "Title Text");
        assert_eq!(tooltip.text, "Description");
        let px = tooltip.icon_pixmap.unwrap();
        assert_eq!(px.width, 1);
        assert_eq!(px.height, 1);
        // big-endian [A,R,G,B] → LE [B,G,R,A]
        assert_eq!(&px.data, &[0x33, 0x22, 0x11, 0xFF]);
    }

    #[test]
    fn tooltip_parse_empty_icon_array() {
        let icon_name: Param = Param::Base(PBase::StringRef(""));
        let pixmaps: Param = Param::Container(Container::Array(rustbus::params::Array {
            element_sig: rustbus::signature::Type::Base(rustbus::signature::Base::Byte),
            values: vec![],
        }));
        let title: Param = Param::Base(PBase::StringRef("T"));
        let text: Param = Param::Base(PBase::StringRef(""));

        let variant: Param = Param::Container(Container::Variant(Box::new(rustbus::params::Variant {
            sig: rustbus::signature::Type::Base(rustbus::signature::Base::String),
            value: Param::Container(Container::Struct(vec![
                icon_name, pixmaps, title, text,
            ])),
        })));

        let tooltip = extract_tooltip_from_param(&variant);
        assert_eq!(tooltip.icon_name, "");
        assert!(tooltip.icon_pixmap.is_none());
        assert_eq!(tooltip.title, "T");
    }

    #[test]
    fn tooltip_parse_wrong_type_returns_default() {
        let variant: Param = Param::Base(PBase::Int32(42));
        let tooltip = extract_tooltip_from_param(&variant);
        assert_eq!(tooltip, ToolTip::default());
    }

    #[test]
    fn tooltip_parse_short_struct_returns_default() {
        // Only 2 fields, need 4
        let variant: Param = Param::Container(Container::Variant(Box::new(rustbus::params::Variant {
            sig: rustbus::signature::Type::Base(rustbus::signature::Base::String),
            value: Param::Container(Container::Struct(vec![
                Param::Base(PBase::StringRef("a")),
                Param::Base(PBase::StringRef("b")),
            ])),
        })));
        let tooltip = extract_tooltip_from_param(&variant);
        assert_eq!(tooltip, ToolTip::default());
    }

    #[test]
    fn tooltip_default_values() {
        let t = ToolTip::default();
        assert_eq!(t.icon_name, "");
        assert!(t.icon_pixmap.is_none());
        assert_eq!(t.title, "");
        assert_eq!(t.text, "");
    }

    #[test]
    fn tooltip_parse_multiple_pixmaps_takes_first() {
        let pixel_1x1: Param = Param::Container(Container::Struct(vec![
            Param::Base(PBase::Int32(1)),
            Param::Base(PBase::Int32(1)),
            Param::Container(Container::Array(rustbus::params::Array {
                element_sig: rustbus::signature::Type::Base(rustbus::signature::Base::Byte),
                values: vec![
                    Param::Base(PBase::Byte(0xAA)),
                    Param::Base(PBase::Byte(0xBB)),
                    Param::Base(PBase::Byte(0xCC)),
                    Param::Base(PBase::Byte(0xDD)),
                ],
            })),
        ]));
        let pixel_2x1: Param = Param::Container(Container::Struct(vec![
            Param::Base(PBase::Int32(2)),
            Param::Base(PBase::Int32(1)),
            Param::Container(Container::Array(rustbus::params::Array {
                element_sig: rustbus::signature::Type::Base(rustbus::signature::Base::Byte),
                values: vec![
                    Param::Base(PBase::Byte(0x11)), Param::Base(PBase::Byte(0x22)),
                    Param::Base(PBase::Byte(0x33)), Param::Base(PBase::Byte(0x44)),
                    Param::Base(PBase::Byte(0x55)), Param::Base(PBase::Byte(0x66)),
                    Param::Base(PBase::Byte(0x77)), Param::Base(PBase::Byte(0x88)),
                ],
            })),
        ]));
        let variant: Param = Param::Container(Container::Variant(Box::new(rustbus::params::Variant {
            sig: rustbus::signature::Type::Base(rustbus::signature::Base::String),
            value: Param::Container(Container::Struct(vec![
                Param::Base(PBase::StringRef("")),
                Param::Container(Container::Array(rustbus::params::Array {
                    element_sig: sig_icon_array(),
                    values: vec![pixel_1x1, pixel_2x1],
                })),
                Param::Base(PBase::StringRef("")),
                Param::Base(PBase::StringRef("")),
            ])),
        })));

        let tooltip = extract_tooltip_from_param(&variant);
        let px = tooltip.icon_pixmap.unwrap();
        // Should take the first pixmap (1x1), not the second (2x1)
        assert_eq!(px.width, 1);
        assert_eq!(px.height, 1);
    }
}
