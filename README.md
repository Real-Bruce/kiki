# kiki

A lightweight, native screenshot and annotation tool for **Windows 11 x64**. Capture a region, mark what matters, and copy, save, or pin the result above other windows. Built with Rust and Windows graphics APIs; just extract the ZIP and run.

**English** · [简体中文](README.zh-CN.md)

[Download](https://github.com/Real-Bruce/kiki/releases) · [Screenshots](#screenshots) · [Documentation](docs/README.md) · [Changelog](docs/CHANGELOG.md)

![kiki capture selection and toolbar, with all tools enabled](docs/screenshots/capture/toolbar-light.png)

## Features

- **Capture:** select a region, a visible window, or the current screen.
- **Annotate:** editable rectangles, arrows, pen strokes, text, mosaic regions, and numbered markers, with undo and redo. Move and rotate text after placing it.
- **Pick colors:** sample the frozen image, switch between HEX and RGB, and copy a color code without leaving the capture.
- **Scrolling capture:** manually scroll a vertical region on one screen, then annotate and export the assembled image.
- **Copy, save, or pin:** copy to the clipboard, save PNG or JPEG, or keep multiple images on top of other windows. Drag and zoom pinned images while preserving their original export size.
- **Make it yours:** choose visible tools, adjust pixel-based annotation sizes and rounded corners, configure the global shortcut, and use light, dark, or system themes.

The application interface is currently in **Simplified Chinese**. Rectangle, arrow, pen, and text tools are enabled by default. Enable mosaic, numbering, scrolling capture, and the color picker in **截图** (Capture settings).

## Get started

Download the Windows x64 ZIP from [Releases](https://github.com/Real-Bruce/kiki/releases), extract it, and launch `kiki.exe`. No Rust toolchain or installer is required.

1. Click **开始截图** (Start capture), press **Ctrl+Alt+A**, or use the tray menu. The global shortcut is configurable.
2. Drag to select a region, or click a highlighted window. Add annotations using the toolbar.
3. Press **Enter** to copy, **Ctrl+S** to save, or **Ctrl+P** to pin. **Esc** cancels the active operation, then the capture when no operation is active.

While entering text, **Enter** inserts a new line and **Ctrl+Enter** finishes editing. Use **Ctrl+Z / Ctrl+Y** for undo / redo outside text editing.

For a pinned image, drag to move, use the wheel to zoom, press **Ctrl+0** to restore its original scale, or **Esc** to close it. Copying and pinning do not automatically save files. Closing Settings leaves kiki in the tray; use the tray menu to exit.

Settings are stored in `%LOCALAPPDATA%\kiki`, independently of the extracted application folder. See the [usage guide (中文)](docs/USAGE.md) for editing controls and output behavior.

Scrolling capture is limited to one screen and manual vertical scrolling. Entering it discards the current annotations, text draft, and undo/redo history, including if capture fails or is canceled. A known stitching issue remains on the Codex in-app browser Daily page. HDR-to-SDR conversion is unfinished; real multi-monitor and mixed-DPI validation remains incomplete. See [current limitations and validation status (中文)](docs/PROGRESS.md).

## Screenshots

### General

Start a capture and manage startup behavior.

![General settings](docs/screenshots/settings/general-light.png)

### Shortcuts

Customize the capture hotkey and find shortcuts for pinned images.

![Shortcut settings](docs/screenshots/settings/shortcuts-light.png)

### Capture

Choose toolbar tools and adjust default annotation styles.

![Capture settings: tool switches, toolbar preview, and annotation defaults](docs/screenshots/settings/capture-light.png)

### Pinned images

Keep captures above other windows, move and zoom them, or close them all at once.

![Pinned image settings](docs/screenshots/settings/pinned-light.png)

### Appearance

Follow the system theme or choose light or dark mode.

![Appearance settings](docs/screenshots/settings/appearance-light.png)

## Development

Build on Windows 11 x64 with MSVC Build Tools, the Windows SDK, and PowerShell 7. The repository pins **Rust 1.88.0**, uses **Rust 2024**, and commits `Cargo.lock`. The native UI uses Win32, Direct2D, and DirectWrite; image encoding uses WIC.

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
cargo test --workspace --all-features --locked -- --test-threads=1
cargo build --release --locked
./scripts/Package-Windows.ps1
```

The executable is written to `target/x86_64-pc-windows-msvc/release/kiki.exe`; portable packages and SHA-256 checksums go to `dist/`. The application version comes from [VERSION](VERSION).

Design, implementation, and validation guides are organized [by module](docs/modules/README.md). See [development and diagnostics](docs/DEVELOPMENT.md), [packaging and releases](docs/RELEASING.md), and [AGENTS.md](AGENTS.md) before contributing. Engineering documents are maintained in Simplified Chinese. When reporting a bug, include your Windows version, display resolution and scaling, reproduction steps, and expected behavior.

## Licenses

Third-party license texts ship with the application and can be read offline from **关于** (About). See [third-party licenses](docs/THIRD_PARTY_LICENSES.md). A license for this project's source code has not yet been declared.
