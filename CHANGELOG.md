# Changelog

## [0.2.0] — 2026-06-24

### Added

- `TrayHost::add_item` for synchronous lookup and insertion of a tray item by bus name.
- `simple_watcher` example: poll-based daemon that runs until Ctrl+C.

### Fixed

- `poll()` now drains **all** complete D-Bus messages per `read_once`, not just one.
- Newly appeared tray items (late `NameOwnerChanged`) are now discovered; previously the watcher could miss them.
- The unique-name probe no longer rejects the queued "watcher" name.

### Changed

- Internal cleanup: removed `scan_blocking`, `menu_dumper`, and dead registration loop code.

## [0.1.1] — 2026-06-24

### Changed

- Switch to upstream `rustbus` 0.19.3.
- Bump MSRV to Rust 1.85 (edition 2024).

### Fixed

- Correct repository URL in `Cargo.toml`.

## [0.1.0] — 2026-06-19

### Added

- Initial release: poll-friendly `StatusNotifierItem` host library.
- `TrayHost` with `new()`, `poll()`, `fd()`, `items()`, `activate()`, `context_menu()`, `secondary_activate()`, `scroll()`, `get_menu()`, `menu_click()`, `provide_xdg_activation_token()`, `shutdown()`.
- `TrayItem` struct with properties and convenience methods.
- `IconPixmap` with ARGB32 conversion.
- ToolTip support.
- `com.canonical.dbusmenu` protocol: layout fetching, property queries, click events.
- `TrayEvent` enum: `ItemAdded`, `ItemRemoved`, `ItemChanged`, `HostShutdown`.
