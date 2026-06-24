/// A StatusNotifierItem watcher.
///
/// Starts a TrayHost, polls periodically, and prints tray events.
/// Runs until the user presses Ctrl+C.
///
/// Usage:
///   cargo run --example simple_watcher

use rustsni::{TrayEvent, TrayHost};
use std::time::{Duration, Instant};

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

    let start = Instant::now();
    let mut items_prev = 0usize;

    loop {
        let items_before = host.items().len();
        match host.poll() {
            Ok(events) => {
                let items_after = host.items().len();
                let dt = start.elapsed().as_secs_f64();

                // Print a summary line whenever the item cache changes,
                // when events arrive, or every 30 seconds as heartbeat.
                if !events.is_empty()
                    || items_after != items_prev
                    || dt as u64 % 30 == 0 && items_after == items_prev
                {
                    items_prev = items_after;
                    if events.is_empty() && items_before == items_after {
                        println!("[{dt:6.1}s] poll: no events, {items_after} item(s) cached");
                    }
                }

                for ev in &events {
                    match ev {
                        TrayEvent::ItemAdded(id) => {
                            let title = host.items().get(id).map(|i| i.title.as_str()).unwrap_or("?");
                            println!("[{dt:6.1}s]  + {id}  \"{title}\"");
                        }
                        TrayEvent::ItemChanged(id) => {
                            let title = host.items().get(id).map(|i| i.title.as_str()).unwrap_or("?");
                            println!("[{dt:6.1}s]  ~ {id}  \"{title}\"");
                        }
                        TrayEvent::ItemRemoved(id) => {
                            println!("[{dt:6.1}s]  - {id}");
                        }
                        TrayEvent::MenuChanged(id) => {
                            println!("[{dt:6.1}s]  ☰ {id} menu changed");
                        }
                        TrayEvent::MenuActivationRequested(id) => {
                            println!("[{dt:6.1}s]  ☰ {id} menu activation requested");
                        }
                        TrayEvent::HostShutdown => {
                            println!("[{dt:6.1}s]  ⏹ host shutdown");
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[{:.1}s] poll error: {e}", start.elapsed().as_secs_f64());
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
