/// A minimal StatusNotifierItem watcher.
///
/// Starts a TrayHost, registers `fd()` with an event loop, and prints
/// tray events as they arrive.  Run it and observe items appearing,
/// changing, and disappearing on your session bus.
///
/// Usage:
///   cargo run --example simple_watcher

use rustsni::{ItemId, TrayEvent, TrayHost};
use std::os::fd::AsRawFd;
use std::time::Duration;

fn main() {
    let mut host = match TrayHost::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: failed to start tray watcher: {e}");
            std::process::exit(1);
        }
    };

    println!("tray host started (fd={})", host.fd().as_raw_fd());

    // In a real application you would register host.fd() with your
    // poll/epoll loop.  Here we simply poll every 200 ms for a few
    // rounds.
    for round in 0..25 {
        match host.poll() {
            Ok(events) => {
                for ev in &events {
                    match ev {
                        TrayEvent::ItemAdded(id) => {
                            if let Some(item) = host.items().get(id) {
                                println!("[{round}]  + {id}  \"{}\"  [{}]{}",
                                    item.title, item.status,
                                    if item.has_menu() { "  ☰" } else { "" },
                                );
                            }
                        }
                        TrayEvent::ItemChanged(id) => {
                            if let Some(item) = host.items().get(id) {
                                println!("[{round}]  ~ {id}  \"{}\"  [{}]", item.title, item.status);
                            }
                        }
                        TrayEvent::ItemRemoved(id) => {
                            println!("[{round}]  - {id}");
                        }
                        TrayEvent::MenuChanged(id) => {
                            println!("[{round}]  ☰ {id} menu changed");
                        }
                        TrayEvent::MenuActivationRequested(id) => {
                            println!("[{round}]  ☰ {id} menu activation requested");
                        }
                        TrayEvent::HostShutdown => {
                            println!("[{round}]  ⏹ host shutdown");
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[{round}] poll error: {e}");
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // ── Interact with the first discovered item ───────────────────
    let first: Option<(ItemId, String)> = host
        .items()
        .iter()
        .map(|(id, item)| (id.clone(), item.menu_path.clone()))
        .next();
    if let Some((id, menu_path)) = first {
        println!("\nactivating first item: {id}");
        let _ = host.activate(&id, 0, 0);
        if !menu_path.is_empty() && menu_path != "/" {
            if let Ok(nodes) = host.get_menu(&id, 0) {
                println!("menu:");
                print_menu(&nodes, 2);
            }
        }
    }

    let _ = host.shutdown();
    println!("done");
}

fn print_menu(nodes: &[rustsni::MenuNode], indent: usize) {
    for node in nodes {
        let pfx = " ".repeat(indent);
        if node.label.is_empty() && node.children.is_empty() {
            println!("{pfx}──────────────────");
            continue;
        }
        let label = if node.enabled {
            node.label.clone()
        } else {
            format!("({})", node.label)
        };
        let toggle = match node.toggle_type.as_str() {
            "checkmark" => format!(" [{}]", node.toggle_state),
            "radio" => format!(" [radio={}]", node.toggle_state),
            _ => String::new(),
        };
        println!("{pfx}{label}{toggle}");
        if !node.children.is_empty() {
            print_menu(&node.children, indent + 2);
        }
    }
}
