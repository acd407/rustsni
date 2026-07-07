# Changelog

## [0.2.2] — 2026-07-07

### Changed

- Remove the active D-Bus probe (`ListNames` + `GetAll`) on startup. The
  watcher now relies exclusively on `StatusNotifierHostRegistered` to
  discover tray items. This eliminates the complex `pending_unique_names`
  retry mechanism and simplifies the poll loop.
- Emit `StatusNotifierHostRegistered` **after** `host::register()`
  completes, so items see a fully-initialized watcher+host before
  attempting to register.
- Buffer unexpected D-Bus messages received during `host::register()` in
  `pending_messages` instead of discarding them, preventing lost
  `RegisterStatusNotifierItem` calls from fast-responding items.

## [0.2.1] — 2026-07-02

### Fixed

- `GetHostServiceName` is now implemented, preventing crashes in tray items that query the host's service name.
- All D-Bus method calls now receive a reply; previously some calls (notably on `/`) would hang the caller until timeout.
- Remove `if-let` chains for MSRV 1.85 compatibility.
- Resolve CI warnings — bump `actions/checkout` to v5, fix MSRV input name.

### Changed

- Remove `pending_unique_names` async probe mechanism to simplify the watcher startup sequence.

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
