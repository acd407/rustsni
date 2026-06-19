//! StatusNotifierHost registration.
//!
//! Just needs to exist on the bus so items know a host is present.

use rustbus::connection::ll_conn::DuplexConn;
use rustbus::connection::Timeout;
use rustbus::standard_messages;

use crate::Result;

/// Register as `org.freedesktop.StatusNotifierHost-{pid}`.
pub fn register(conn: &mut DuplexConn) -> Result<()> {
    let name = format!(
        "org.freedesktop.StatusNotifierHost-{}",
        std::process::id()
    );
    let msg = standard_messages::request_name(&name, 0);
    conn.send.send_message_write_all(&msg)?;
    // Skip signals until we get the reply
    loop {
        let resp = conn.recv.get_next_message(Timeout::Infinite)?;
        if resp.typ != rustbus::message_builder::MessageType::Signal {
            break;
        }
    }
    Ok(())
}
