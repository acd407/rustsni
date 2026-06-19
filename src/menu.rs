//! `com.canonical.dbusmenu` support.

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::connection::Timeout;
use rustbus::params::{Base, Container, Param};

use crate::Result;

/// A menu node from GetLayout.
#[derive(Debug, Clone)]
pub struct MenuNode {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub is_submenu: bool,
    pub children: Vec<MenuNode>,
}

/// Call `GetLayout(parent_id, 1, &[])` and parse the children.
pub fn get_layout(
    conn: &mut DuplexConn,
    bus_name: &str,
    menu_path: &str,
    parent_id: i32,
) -> Result<Vec<MenuNode>> {
    let mut call = rustbus::MessageBuilder::new()
        .call("GetLayout")
        .on(menu_path)
        .with_interface("com.canonical.dbusmenu")
        .at(bus_name)
        .build();
    call.body.push_param(parent_id).unwrap();
    call.body.push_param(1i32).unwrap(); // recursion_depth
    call.body.push_param(Vec::<&str>::new()).unwrap(); // all properties

    let serial = conn.send.send_message_write_all(&call)?;
    // Skip signals until reply
    let resp = loop {
        let resp = conn.recv.get_next_message(Timeout::Infinite)?;
        if resp.typ == rustbus::message_builder::MessageType::Reply {
            break resp;
        }
        if resp.typ == rustbus::message_builder::MessageType::Error {
            return Ok(Vec::new());
        }
    };
    if resp.dynheader.response_serial != Some(serial) {
        return Ok(Vec::new());
    }

    // Response is (u32 revision, (i32 id, a{sv} props, av children))
    let mut parser = resp.body.parser();
    let _revision: u32 = parser.get()?;
    let root = match parser.get_param() {
        Ok(p) => p,
        Err(_) => return Ok(Vec::new()),
    };

    // root is a struct (i32, a{sv}, av)
    let fields = match &root {
        Param::Container(Container::Struct(s)) => s,
        _ => return Ok(Vec::new()),
    };
    if fields.len() < 3 {
        return Ok(Vec::new());
    }

    // children is the third field: av (array of variants)
    let children = match &fields[2] {
        Param::Container(Container::Array(arr)) => &arr.values,
        _ => return Ok(Vec::new()),
    };

    Ok(children.iter().filter_map(parse_menu_node).collect())
}

/// Fire a menu click event via a fresh D-Bus connection.
pub fn fire_click(bus_name: &str, menu_path: &str, menu_id: i32) -> Result<()> {
    let mut conn = rustbus::connection::ll_conn::DuplexConn::connect_to_bus(
        rustbus::get_session_bus_path()?,
        false,
    )?;
    conn.send_hello(rustbus::connection::Timeout::Infinite)?;
    event(&mut conn, bus_name, menu_path, menu_id, "clicked")
}

/// Fire an event on a menu item.
pub fn event(
    conn: &mut DuplexConn,
    bus_name: &str,
    menu_path: &str,
    id: i32,
    event_id: &str,
) -> Result<()> {
    let mut call = rustbus::MessageBuilder::new()
        .call("Event")
        .on(menu_path)
        .with_interface("com.canonical.dbusmenu")
        .at(bus_name)
        .build();
    call.body.push_param(id).unwrap();
    call.body.push_param(event_id).unwrap();
    // data: variant containing i32(0)
    call.body.push_variant(0i32).unwrap();
    call.body.push_param(0u32).unwrap(); // timestamp

    conn.send.send_message_write_all(&call)?;
    // Don't wait for reply - fire and forget
    Ok(())
}

/// Parse a variant containing (i32, a{sv}, av) into a MenuNode.
fn parse_menu_node(param: &Param) -> Option<MenuNode> {
    let variant_inner = match param {
        Param::Container(Container::Variant(v)) => &v.value,
        _ => return None,
    };

    // variant_inner is Param - match it directly
    let fields = match variant_inner {
        Param::Container(Container::Struct(s)) => s.as_slice(),
        _ => return None,
    };
    if fields.len() < 3 {
        return None;
    }

    let id = match &fields[0] {
        Param::Base(Base::Int32(v)) => *v,
        _ => return None,
    };

    let props = match &fields[1] {
        Param::Container(Container::Dict(d)) => &d.map,
        _ => return None,
    };

    let label = get_str_prop(props, "label");
    let enabled = get_bool_prop(props, "enabled").unwrap_or(true);
    let visible = get_bool_prop(props, "visible").unwrap_or(true);
    let is_submenu = props.iter().any(|(k, _)| {
        match k {
            Base::StringRef(s) => *s == "children-display",
            Base::String(s) => s == "children-display",
            _ => false,
        }
    });

    let children_param = match &fields[2] {
        Param::Container(Container::Array(arr)) => &arr.values,
        _ => return None,
    };
    let children: Vec<MenuNode> = children_param.iter().filter_map(parse_menu_node).collect();

    Some(MenuNode {
        id,
        label,
        enabled,
        visible,
        is_submenu: is_submenu || !children.is_empty(),
        children,
    })
}

fn get_str_prop(props: &rustbus::params::DictMap, key: &str) -> String {
    for (k, v) in props {
        let k_str = match k {
            Base::StringRef(s) => *s,
            Base::String(s) => s.as_str(),
            _ => continue,
        };
        if k_str != key {
            continue;
        }
        if let Param::Container(Container::Variant(var)) = v {
            match &var.value {
                Param::Base(Base::StringRef(s)) => return s.to_string(),
                Param::Base(Base::String(s)) => return s.clone(),
                _ => {}
            }
        }
    }
    String::new()
}

fn get_bool_prop(props: &rustbus::params::DictMap, key: &str) -> Option<bool> {
    for (k, v) in props {
        let k_str = match k {
            Base::StringRef(s) => *s,
            Base::String(s) => s.as_str(),
            _ => continue,
        };
        if k_str != key {
            continue;
        }
        if let Param::Container(Container::Variant(var)) = v {
            if let Param::Base(Base::Boolean(b)) = &var.value {
                return Some(*b);
            }
        }
    }
    None
}
