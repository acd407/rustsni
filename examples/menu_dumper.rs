/// Dump a tray item's menu tree.
///
/// Connects to the session bus, discovers tray items, and prints the
/// menu layout of a specific item matched by `--id` or `--address`.
///
/// Usage:
///   cargo run --example menu_dumper -- --id wechat
///   cargo run --example menu_dumper -- --address :1.60
///   cargo run --example menu_dumper -- --address org.kde.StatusNotifierItem-2-1

use rustsni::{ItemId, TrayHost};
use std::time::{Duration, Instant};

fn main() {
    let (filter_key, filter_val) = parse_args_or_exit();

    let mut host = match TrayHost::new() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: failed to start tray watcher: {e}");
            std::process::exit(1);
        }
    };

    // ── Locate the item ───────────────────────────────────────────
    // When the user provides a D-Bus address we can fetch the item
    // immediately without waiting for async discovery.
    let item_id = match filter_key {
        "address" => {
            match host.add_item(&filter_val, "/StatusNotifierItem") {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("error: cannot fetch item at {filter_val}: {e}");
                    std::process::exit(1);
                }
            }
        }
        "id" => {
            // We need to know the bus name to fetch directly, so
            // fall back to async discovery.  The library probes one
            // unique name per poll() call, so this may take a while
            // on a busy bus.
            let start = Instant::now();
            loop {
                if let Some(id) = find_item_by_id(&host, &filter_val) {
                    break id;
                }
                let _ = host.poll();
                if start.elapsed() > Duration::from_secs(10) {
                    eprintln!("error: item id \"{filter_val}\" not found within 10 s");
                    eprintln!("  known items:");
                    for (id, item) in host.items() {
                        eprintln!("    {id}  id=\"{}\"  bus=\"{}\"",
                            item.item_id, item.bus_name);
                    }
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        _ => unreachable!(),
    };

    // ── Fetch and print menu ──────────────────────────────────────
    let item = &host.items()[&item_id];
    if !item.has_menu() {
        println!(
            "item \"{}\" (at {}) has no menu",
            item.item_id, item.bus_name,
        );
        return;
    }

    println!(
        "menu for {} (id=\"{}\", path={}):",
        item_id, item.item_id, item.menu_path,
    );

    match host.get_menu(&item_id, 0) {
        Ok(nodes) => {
            if nodes.is_empty() {
                println!("  (empty menu tree)");
            } else {
                print_nodes(&nodes, 2);
            }
        }
        Err(e) => {
            eprintln!("error getting menu: {e}");
        }
    }
}

// ── Arg parsing ─────────────────────────────────────────────────

fn parse_args_or_exit() -> (&'static str, String) {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: cargo run --example menu_dumper -- --id <item_id>");
        eprintln!("       cargo run --example menu_dumper -- --address <bus_name>");
        std::process::exit(1);
    }
    let val = args[2].clone();
    match args[1].as_str() {
        "--id" => ("id", val),
        "--address" => ("address", val),
        flag => {
            eprintln!("unknown flag: {flag}");
            std::process::exit(1);
        }
    }
}

// ── Item lookup ─────────────────────────────────────────────────

fn find_item_by_id(host: &TrayHost, target: &str) -> Option<ItemId> {
    for (id, item) in host.items() {
        if item.item_id == target {
            return Some(id.clone());
        }
    }
    None
}

// ── Menu printer ────────────────────────────────────────────────

fn print_nodes(nodes: &[rustsni::MenuNode], indent: usize) {
    for node in nodes {
        let pfx = " ".repeat(indent);

        // Separator: empty leaf items get a divider line.
        if node.label.is_empty() && node.children.is_empty() {
            println!("{pfx}──────────────────");
            continue;
        }

        // Label: grey out disabled items.
        let label = if node.enabled {
            node.label.clone()
        } else {
            format!("({})", node.label)
        };

        // Toggle info.
        let toggle = match node.toggle_type.as_str() {
            "checkmark" => format!(" [toggle={}]", node.toggle_state),
            "radio" => format!(" [radio={}]", node.toggle_state),
            _ => String::new(),
        };

        println!("{pfx}{label}{toggle}");

        // Recurse into children.
        if !node.children.is_empty() {
            print_nodes(&node.children, indent + 2);
        }
    }
}
