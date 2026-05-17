# loki-file-access

Cross-platform, frontend-agnostic file picker and capability-based file access for Rust.

## Features

- **Desktop** (Windows, macOS, Linux, BSD) — via the [`rfd`](https://crates.io/crates/rfd) crate
- **Android** — via the Storage Access Framework with persistable URI permissions
- **iOS** — via `UIDocumentPickerViewController` with security-scoped bookmarks
- **WASM** — via `<input type="file">` with in-memory file buffers

No UI framework dependencies. Returns standard `Future` values built on `std` primitives — usable from Dioxus, egui, Iced, `pollster::block_on`, or any other async or sync context.

## Quick start

```toml
[dependencies]
loki-file-access = "0.1.2"
```

```rust
use loki_file_access::{FilePicker, PickOptions};
use std::io::Read;

let picker = FilePicker::new();
let token = picker
    .pick_file_to_open(PickOptions {
        mime_types: vec!["text/plain".into()],
        ..Default::default()
    })
    .await?;

if let Some(token) = token {
    let mut reader = token.open_read()?;
    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;
}
```

## Linux requirements

On Linux, loki-file-access uses the [XDG Desktop Portal](https://flatpak.github.io/xdg-desktop-portal/) (`org.freedesktop.portal.FileChooser`) for file dialogs, accessed via D-Bus.

Most desktop Linux environments (GNOME, KDE, etc.) provide this portal automatically. If the portal is not running, the file picker will silently return `None` — enable a `tracing` subscriber in your application to see the warning message that explains this.

### ChromeOS Crostini

The XDG Desktop Portal is **not** available inside the Crostini Linux container. As a result, the file picker will not open a dialog on ChromeOS Crostini with the current version of `rfd`.

A future version of this crate may add a GTK3/zenity fallback for environments without the portal. For now, ChromeOS Crostini users will see a `tracing::warn!` message instead of a dialog.

### Troubleshooting

**The file picker does nothing on my Linux system.**

1. Ensure D-Bus is running and the XDG Desktop Portal is installed:
   ```sh
   # Debian/Ubuntu
   sudo apt install xdg-desktop-portal xdg-desktop-portal-gtk

   # Fedora
   sudo dnf install xdg-desktop-portal xdg-desktop-portal-gtk

   # Arch
   sudo pacman -S xdg-desktop-portal xdg-desktop-portal-gtk
   ```

2. Enable a `tracing` subscriber in your app (e.g. `tracing_subscriber::fmt::init()`) to see diagnostic messages from the picker.

3. If you want to explicitly disable the picker and surface a clear error instead of a silent no-op, set the environment variable:
   ```sh
   LOKI_FILE_ACCESS_BACKEND=none ./your-app
   ```
   Valid values: `auto` (default), `none`.

## License

MIT — see [LICENSE](LICENSE).
