/// A StatusNotifierItem watcher.
///
/// Starts a TrayHost, registers `fd()` with an event loop (or in this
/// case a simple poll loop), and prints tray events as they arrive.
/// Runs until the user presses Ctrl+C.
///
/// Usage:
///   cargo run --example simple_watcher

use rustsni::{TrayEvent, TrayHost};

fn main() {
    let mut host = match TrayHost::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: failed to start tray watcher: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "tray host started (fd={}) — press Ctrl+C to stop",
        host.fd(),
    );

    // In a real application you would register host.fd() with your
    // poll/epoll loop.  Here we simply poll at a fixed interval.
    loop {
        match host.poll() {
            Ok(events) => {
                for ev in &events {
                    match ev {
                        TrayEvent::ItemAdded(id) => {
                            if let Some(item) = host.items().get(id) {
                                println!("+ {id}  \"{}\"  [{}]{}",
                                    item.title, item.status,
                                    if item.has_menu() { "  ☰" } else { "" },
                                );
                            }
                        }
                        TrayEvent::ItemChanged(id) => {
                            if let Some(item) = host.items().get(id) {
                                println!("~ {id}  \"{}\"  [{}]", item.title, item.status);
                            }
                        }
                        TrayEvent::ItemRemoved(id) => {
                            println!("- {id}");
                        }
                        TrayEvent::MenuChanged(id) => {
                            println!("☰ {id} menu changed");
                        }
                        TrayEvent::MenuActivationRequested(id) => {
                            println!("☰ {id} menu activation requested");
                        }
                        TrayEvent::HostShutdown => {
                            println!("⏹ host shutdown");
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("poll error: {e}");
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
