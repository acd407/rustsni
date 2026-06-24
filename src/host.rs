//! StatusNotifierHost registration.
//!
//! Just needs to exist on the bus so items know a host is present.

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::standard_messages;

use crate::Result;

/// Register as `org.freedesktop.StatusNotifierHost-{pid}`.
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
