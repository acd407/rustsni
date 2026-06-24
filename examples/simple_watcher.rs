/// A minimal StatusNotifierItem watcher.
///
/// Starts a TrayHost, polls the session bus to discover tray items, and
/// prints their properties. Keeps polling until at least one item is
/// found, then interacts with it and exits.
///
/// Usage:
///   cargo run --example simple_watcher

use rustsni::{ItemId, TrayEvent, TrayHost};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

fn main() {
    let mut host = match TrayHost::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: failed to start tray watcher: {e}");
            eprintln!("  (is a D-Bus session bus running?)");
            std::process::exit(1);
        }
    };

    println!("tray host started (fd={})", host.fd().as_raw_fd());

    // ── Discovery ──────────────────────────────────────────────────
    // The library discovers already-running items by probing D-Bus
    // unique names (:1.xxx) one per poll() call — an async, non-blocking
    // handshake. On a busy bus with many services this may take dozens
    // or hundreds of rounds to reach the names that host tray items.
    //
    // Loop until we find something or hit a timeout.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_progress = String::new();

    loop {
        flush_events(&mut host);
        let known = host.items().len();

        if known > 0 {
            println!("\nfound {known} tray item(s):");
            for (id, item) in host.items() {
                print_item(id, item);
            }
            break;
        }

        // Print a progress dot every ~2 s so the user knows we're probing.
        let elapsed = Instant::now().duration_since(deadline - Duration::from_secs(10));
        let tick = format!("{:4}s", elapsed.as_secs());
        if tick != last_progress {
            last_progress = tick.clone();
            print!("\r  probing… {tick}");
            std::io::Write::flush(&mut std::io::stdout()).ok();
        }

        if Instant::now() >= deadline {
            println!("\n  timeout — no tray items discovered.");
            println!("  (items may have registered after the probe window, or");
            println!("   this bus may have no SNI providers running.)");
            break;
        }

        // Give D-Bus time to deliver the next probe reply.
        std::thread::sleep(Duration::from_millis(50));
    }

    println!();

    // ── Interaction ────────────────────────────────────────────────
    // Activate the first discovered item and dump its menu tree.
    let first: Option<(ItemId, String)> = host
        .items()
        .iter()
        .next()
        .map(|(id, item)| (id.clone(), item.menu_path.clone()));

    if let Some((id, menu_path)) = first {
        println!("activating first item: {id}");
        let _ = host.activate(&id, 0, 0);

        if !menu_path.is_empty() && menu_path != "/" {
            println!("  menu at {menu_path}");
            if let Ok(nodes) = host.get_menu(&id, 0) {
                print_menu(&nodes, 2);
            }
        }
    }

    // ── Shutdown ───────────────────────────────────────────────────
    let _ = host.shutdown();
    println!("done");
}

/// Poll for events and print them.
fn flush_events(host: &mut TrayHost) -> bool {
    match host.poll() {
        Ok(events) => {
            let n = events.len();
            for ev in &events {
                match ev {
                    TrayEvent::ItemAdded(id) => println!("\n  + {id} added"),
                    TrayEvent::ItemChanged(id) => println!("\n  ~ {id} changed"),
                    TrayEvent::ItemRemoved(id) => println!("\n  - {id} removed"),
                    TrayEvent::MenuChanged(id) => println!("\n  ☰ {id} menu changed"),
                    TrayEvent::MenuActivationRequested(id) => {
                        println!("\n  ☰ {id} menu activation requested")
                    }
                    TrayEvent::HostShutdown => println!("\n  ⏹ host shutdown"),
                }
            }
            n > 0
        }
        Err(e) => {
            eprintln!("\npoll error: {e}");
            false
        }
    }
}

/// Print a summary line for a tray item.
fn print_item(id: &ItemId, item: &rustsni::TrayItem) {
    print!("  {id:<55}");
    if let Some(best) = item.best_icon_pixmap() {
        print!(" icon={}x{}", best.width, best.height);
    } else if !item.icon_name.is_empty() {
        print!(" icon=\"{}\"", item.icon_name);
    }
    if !item.title.is_empty() {
        print!("  \"{}\"", item.title);
    }
    print!("  [{}]", item.status);
    if item.has_menu() {
        print!("  ☰");
    }
    println!();
}

/// Recursively print a menu node tree.
fn print_menu(nodes: &[rustsni::MenuNode], indent: usize) {
    for node in nodes {
        let pfx = " ".repeat(indent);
        let label = if node.enabled {
            node.label.clone()
        } else {
            format!("({})", node.label)
        };
        println!("{pfx}{label}");
        if !node.children.is_empty() {
            print_menu(&node.children, indent + 2);
        }
    }
}
