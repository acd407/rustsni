# rustsni

A poll-friendly [StatusNotifierItem](https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/) host library built on `rustbus`.

Provides a `StatusNotifierWatcher` that discovers SNI items on the D-Bus session bus, and types for interacting with their properties, icons, and menus — all in a non-blocking, poll-friendly style.

## Usage

```rust
use rustsni::StatusNotifierWatcher;

let mut watcher = StatusNotifierWatcher::new()?;
loop {
    watcher.poll(|_host_id, item| {
        println!("item: {} ({})", item.id().item_id(), item.title());
    })?;
}
```

## License

MIT
