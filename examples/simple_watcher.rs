/// A minimal StatusNotifierItem watcher.
///
/// Starts a TrayHost, discovers tray items, prints their properties,
/// then interacts with the first one and exits.
///
/// Uses [`TrayHost::scan_blocking`] for fast initial discovery — probes
/// all D-Bus unique names synchronously (hundreds of names in under a
/// second on a typical system).
///
/// Usage:
///   cargo run --example simple_watcher

use rustsni::{ItemId, TrayHost};
use std::time::Duration;

fn main() {
    let mut host = match TrayHost::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: failed to start tray watcher: {e}");
            eprintln!("  (is a D-Bus session bus running?)");
            std::process::exit(1);
        }
    };

    // ── Fast scan ─────────────────────────────────────────────────
    // Scan all pending unique names synchronously, 200 ms per name.
    // On a typical bus with ~100 names this finishes in a few seconds at
    // most; most names return an error immediately (not SNI items), so
    // the real wall-clock time is usually much less.
    let found = match host.scan_blocking(200) {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("scan error: {e}");
            Vec::new()
        }
    };

    // Continue polling for items that may register after our scan
    // (e.g. lazy-starting applications).  Give them a short window.
    if found.is_empty() {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if let Ok(events) = host.poll() {
                for ev in &events {
                    if matches!(ev, rustsni::TrayEvent::ItemAdded(_)) {
                        // found via async registration
                    }
                }
                if !host.items().is_empty() {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // ── Print items ───────────────────────────────────────────────
    let items = host.items();
    if items.is_empty() {
        println!("no tray items found.");
        return;
    }

    println!("found {} tray item(s):", items.len());
    for (id, item) in items {
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

    // ── Interact with first item ──────────────────────────────────
    let first: Vec<(ItemId, String)> = host
        .items()
        .iter()
        .take(1)
        .map(|(id, item)| (id.clone(), item.menu_path.clone()))
        .collect();

    if let Some((id, menu_path)) = first.into_iter().next() {
        println!("\nactivating: {id}");
        let _ = host.activate(&id, 0, 0);
        if !menu_path.is_empty() && menu_path != "/" {
            if let Ok(nodes) = host.get_menu(&id, 0) {
                println!("menu:");
                print_menu(&nodes, 2);
            }
        }
    }

    // ── Shutdown ──────────────────────────────────────────────────
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
