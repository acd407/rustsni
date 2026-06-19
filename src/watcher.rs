//! StatusNotifierWatcher implementation.
//!
//! Registers on the session bus and handles item registration/deregistration.

use std::collections::HashMap;

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::connection::Timeout;
use rustbus::message_builder::MarshalledMessage;
use rustbus::standard_messages;

use crate::item::{ItemId, TrayItem};
use crate::{Result, TrayEvent};

const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// Request the watcher bus names and set up signal subscriptions.
pub fn register(conn: &mut DuplexConn) -> Result<()> {
    // Request both the freedesktop and KDE well-known names
    for name in [
        "org.freedesktop.StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher",
    ] {
        let msg = standard_messages::request_name(name, 0);
        conn.send.send_message_write_all(&msg)?;
        // Skip signals until we get the reply
        loop {
            let resp = conn.recv.get_next_message(Timeout::Infinite)?;
            if resp.typ == rustbus::message_builder::MessageType::Reply {
                break;
            }
            if resp.typ == rustbus::message_builder::MessageType::Error {
                return Err(crate::Error::WatcherAlreadyRunning);
            }
        }
    }

    // Subscribe to NameOwnerChanged so we know when items vanish
    let add_match = standard_messages::add_match(
        "type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged'",
    );
    conn.send.send_message_write_all(&add_match)?;

    // Emit StatusNotifierHostRegistered to notify existing items
    eprintln!("rustsni: emitting StatusNotifierHostRegistered signal");
    let sig = rustbus::MessageBuilder::new()
        .signal(WATCHER_INTERFACE, "StatusNotifierHostRegistered", WATCHER_PATH)
        .build();
    conn.send.send_message_write_all(&sig)?;
    eprintln!("rustsni: signal sent");

    Ok(())
}

/// Handle an incoming method call.
pub fn handle_call(
    conn: &mut DuplexConn,
    msg: &MarshalledMessage,
    items: &mut HashMap<ItemId, TrayItem>,
    events: &mut Vec<TrayEvent>,
) -> Result<()> {
    let iface = msg.dynheader.interface.as_deref().unwrap_or("");
    let member = msg.dynheader.member.as_deref().unwrap_or("");
    let object = msg.dynheader.object.as_deref().unwrap_or("");

    if object != WATCHER_PATH {
        return Ok(());
    }

    match iface {
        WATCHER_INTERFACE => handle_watcher_call(conn, msg, member, items, events),
        "org.freedesktop.DBus.Properties" => handle_properties_call(conn, msg, member, items),
        "org.freedesktop.DBus.Introspectable" => {
            // Silently ignore introspection for now
            let reply = msg.dynheader.make_response();
            conn.send.send_message_write_all(&reply)?;
            Ok(())
        }
        _ => {
            let err = standard_messages::unknown_method(&msg.dynheader);
            conn.send.send_message_write_all(&err)?;
            Ok(())
        }
    }
}

fn handle_watcher_call(
    conn: &mut DuplexConn,
    msg: &MarshalledMessage,
    member: &str,
    items: &mut HashMap<ItemId, TrayItem>,
    events: &mut Vec<TrayEvent>,
) -> Result<()> {
    match member {
        "RegisterStatusNotifierItem" => {
            let service: String = msg.body.parser().get()?;
            let sender = msg.dynheader.sender.as_deref().unwrap_or("");

            let (bus_name, object_path) = parse_service(sender, &service);

            // Build service_id: sender+path if only a path was given, else the raw service
            let service_id = if service.starts_with('/') {
                format!("{sender}{service}")
            } else {
                service.clone()
            };

            // Read item properties from the bus
            eprintln!("rustsni: registering item {service_id:?}, bus={bus_name:?}, path={object_path:?}");
            match TrayItem::from_bus_with_path(conn, &service_id, &bus_name, &object_path) {
                Ok(item) => {
                    eprintln!("rustsni: item {service_id:?} registered successfully");
                    let id = item.id.clone();
                    items.insert(id.clone(), item);
                    events.push(TrayEvent::ItemAdded(id));
                }
                Err(e) => {
                    eprintln!("rustsni: failed to read item {bus_name}: {e}");
                }
            }

            // Send empty reply
            let reply = msg.dynheader.make_response();
            conn.send.send_message_write_all(&reply)?;
        }
        "RegisterStatusNotifierHost" => {
            let reply = msg.dynheader.make_response();
            conn.send.send_message_write_all(&reply)?;
        }
        _ => {
            let err = standard_messages::unknown_method(&msg.dynheader);
            conn.send.send_message_write_all(&err)?;
        }
    }
    Ok(())
}

fn handle_properties_call(
    conn: &mut DuplexConn,
    msg: &MarshalledMessage,
    member: &str,
    items: &mut HashMap<ItemId, TrayItem>,
) -> Result<()> {
    match member {
        "Get" => {
            let mut parser = msg.body.parser();
            let iface: &str = parser.get()?;
            let prop: &str = parser.get()?;

            // Only handle properties for our watcher interface
            if iface != WATCHER_INTERFACE {
                let err = msg.dynheader.make_error_response(
                    "org.freedesktop.DBus.Error.UnknownProperty".to_owned(),
                    Some(format!("Unknown interface: {iface}")),
                );
                conn.send.send_message_write_all(&err)?;
                return Ok(());
            }

            let mut reply = msg.dynheader.make_response();
            match prop {
                "RegisteredStatusNotifierItems" => {
                    let names: Vec<&str> = items.keys().map(|id| id.0.as_str()).collect();
                    reply.body.push_variant(names).unwrap();
                }
                "IsStatusNotifierHostRegistered" => {
                    reply.body.push_variant(true).unwrap();
                }
                "ProtocolVersion" => {
                    reply.body.push_variant(0i32).unwrap();
                }
                _ => {
                    let err = msg.dynheader.make_error_response(
                        "org.freedesktop.DBus.Error.UnknownProperty".to_owned(),
                        Some(format!("Unknown property: {prop}")),
                    );
                    conn.send.send_message_write_all(&err)?;
                    return Ok(());
                }
            }
            conn.send.send_message_write_all(&reply)?;
        }
        "GetAll" => {
            // Return an error — clients should fall back to Get()
            let err = msg.dynheader.make_error_response(
                "org.freedesktop.DBus.Error.NotSupported".to_owned(),
                Some("GetAll not supported, use Get".to_owned()),
            );
            conn.send.send_message_write_all(&err)?;
        }
        _ => {
            let err = standard_messages::unknown_method(&msg.dynheader);
            conn.send.send_message_write_all(&err)?;
        }
    }
    Ok(())
}

/// Handle incoming signals (NameOwnerChanged, item property-change signals).
pub fn handle_signal(
    conn: &mut DuplexConn,
    msg: &MarshalledMessage,
    items: &mut HashMap<ItemId, TrayItem>,
    events: &mut Vec<TrayEvent>,
) -> Result<()> {
    let iface = msg.dynheader.interface.as_deref().unwrap_or("");
    let member = msg.dynheader.member.as_deref().unwrap_or("");
    eprintln!("rustsni: signal received: iface={iface:?}, member={member:?}");

    if iface == "org.freedesktop.DBus" && member == "NameOwnerChanged" {
        let mut parser = msg.body.parser();
        let name: String = parser.get()?;
        let _old_owner: String = parser.get()?;
        let new_owner: String = parser.get()?;

        // Find items whose bus_name matches the gone name
        if new_owner.is_empty() {
            let ids_to_remove: Vec<ItemId> = items
                .values()
                .filter(|item| item.bus_name == name)
                .map(|item| item.id.clone())
                .collect();
            for id in ids_to_remove {
                items.remove(&id);
                events.push(TrayEvent::ItemRemoved(id));
            }
        }
    } else if iface == "org.kde.StatusNotifierItem" {
        let sender = msg.dynheader.sender.as_deref().unwrap_or("");
        if !sender.is_empty() {
            match TrayItem::from_bus(conn, sender) {
                Ok(item) => {
                    let id = item.id.clone();
                    items.insert(id.clone(), item);
                    events.push(TrayEvent::ItemChanged(id));
                }
                Err(e) => {
                    eprintln!("rustsni: failed to re-read item {sender}: {e}");
                }
            }
        }
    }

    Ok(())
}

/// Split a raw SNI service registration string into `(bus_name, object_path)`.
///
/// The SNI spec allows three forms:
/// - `"com.example.App"` → bus_name, default path `/StatusNotifierItem`
/// - `"/StatusNotifierItem"` → sender's bus name, given path
/// - `"com.example.App/SomePath"` → bus_name, given path
fn parse_service(sender: &str, service: &str) -> (String, String) {
    if service.starts_with('/') {
        (sender.to_owned(), service.to_owned())
    } else if let Some(slash) = service.find('/') {
        (service[..slash].to_owned(), service[slash..].to_owned())
    } else {
        (normalize_item_name(service), "/StatusNotifierItem".to_owned())
    }
}

/// Normalize the service name passed to RegisterStatusNotifierItem.
fn normalize_item_name(name: &str) -> String {
    if name.starts_with("org.freedesktop.StatusNotifierItem-")
        || name.starts_with("org.kde.StatusNotifierItem-")
        || name.starts_with(':')
    {
        return name.to_owned();
    }
    format!("org.freedesktop.StatusNotifierItem-{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_full_name() {
        let input = "org.freedesktop.StatusNotifierItem-4077-1";
        assert_eq!(normalize_item_name(input), input);
    }

    #[test]
    fn normalize_unique_name() {
        assert_eq!(normalize_item_name(":1.42"), ":1.42");
    }

    #[test]
    fn normalize_pid_id() {
        assert_eq!(
            normalize_item_name("4077-1"),
            "org.freedesktop.StatusNotifierItem-4077-1"
        );
    }
}
