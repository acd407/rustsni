//! StatusNotifierHost registration.
//!
//! Per the [SNI spec], a `StatusNotifierHost` is an object in an application
//! that does the actual graphical representation of tray items. It doesn't
//! need any particular methods or signals on the bus — just the presence of
//! the well-known name `org.freedesktop.StatusNotifierHost-{pid}` is enough
//! to signal items that a graphical shell is running.
//!
//! If no host is registered, SNI items may fall back to the X11 system tray
//! protocol (XDG shell / `_NET_SYSTEM_TRAY`).
//!
//! [SNI spec]: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::standard_messages;

use crate::Result;

/// Register as `org.freedesktop.StatusNotifierHost-{pid}` on the session bus.
///
/// This acquires a unique well-known name so tray items know a host is
/// present. The name is automatically released when the D-Bus connection
/// is dropped.
pub fn register(conn: &mut DuplexConn) -> Result<()> {
    let name = format!("org.freedesktop.StatusNotifierHost-{}", std::process::id());
    let msg = standard_messages::request_name(&name, 0);
    let serial = conn.send.send_message_write_all(&msg)?;
    // Wait for the reply matching our serial; skip unrelated signals.
    loop {
        let resp = conn
            .recv
            .get_next_message(rustbus::connection::Timeout::Infinite)?;
        if resp.dynheader.response_serial == Some(serial) {
            // Reply or Error — either way the name was claimed or already held
            break;
        }
        if !matches!(resp.typ, rustbus::message_builder::MessageType::Signal) {
            break; // unexpected — abort spin
        }
    }
    Ok(())
}
