# Wrusp

**Español**: [README.md](README.md)

**Unofficial** WhatsApp desktop client, written in **Rust** with [Tauri 2](https://tauri.app). Built for Linux, with Windows and macOS builds too.

Wrusp wraps WhatsApp Web in a native webview and adds what the web version doesn't offer on the desktop:

- **One window with a sidebar** — every account lives in the same window; switch between them with a click, without reloading the session.
- **Multi-account** — each account keeps a fully isolated session (separate webview profiles).
- **Video plays inside the chat** — codecs come from your system through GStreamer.
- **Voice notes and camera** — the view gets the capture permissions the engine ships disabled.
- **Desktop notifications** — with sender and message, including the ones WhatsApp fires from its service worker.
- **Drag and drop** — drop a file onto a chat to send it.
- **Unread counter** — badge on the tray and taskbar icon, and per account in the sidebar.
- **Keyboard shortcuts** — `Ctrl`+`1`…`9` to switch accounts, `Ctrl`+`U` to add one, `Ctrl`+`P` for settings, plus per-account zoom that is remembered.
- **System tray** — closing the window hides it; Wrusp keeps receiving messages from the tray.
- **Light / dark / system theme** — applied to the app and to WhatsApp Web itself.
- **Single instance** — launching the binary again focuses the existing window.
- **Inspectable log** — the app, the engine and the WhatsApp Web console write
  to a log file, with the folder configurable from settings.
- **No Node** — the settings frontend is plain embedded HTML/CSS/JS.

> ⚠️ Wrusp is not affiliated with, associated with, or endorsed by WhatsApp or
> Meta. It uses WhatsApp Web internally: the same terms of service that would
> apply in your browser apply here.

## Installation

Download the package for your distribution from [Releases](https://github.com/Aleixenandros/Wrusp/releases):

| System | Package |
| --- | --- |
| Debian / Ubuntu / Mint | `.deb` |
| Fedora / openSUSE | `.rpm` |
| Arch / Manjaro | `.pkg.tar.zst` |
| Other Linux distros | `.AppImage` |
| Windows | `.msi` / `.exe` installer |
| macOS (Apple Silicon) | `.dmg` |

Windows and macOS binaries are unsigned: SmartScreen will warn on Windows, and
on macOS you need to clear the quarantine flag after installing
(`xattr -dr com.apple.quarantine /Applications/Wrusp.app`).

Every release ships `SHA256SUMS.txt` with a GitHub provenance attestation:

```bash
gh attestation verify SHA256SUMS.txt --repo Aleixenandros/Wrusp
sha256sum -c SHA256SUMS.txt --ignore-missing
```

> **GNOME**: you need the
> [AppIndicator](https://extensions.gnome.org/extension/615/appindicator-support/)
> extension for the tray icon to show up.

### Video and audio

The engine plays whatever GStreamer can decode on your system. WhatsApp sends
H.264 video with AAC audio, so if a video won't start, install the matching
plugins:

```bash
# Fedora — requires RPM Fusion (https://rpmfusion.org/Configuration):
# Fedora's own openh264 only decodes the baseline profile, and many
# WhatsApp videos use Main or High. gstreamer1-plugin-libav is the plugin
# that exposes the full decoders (it is not always preinstalled);
# libavcodec-freeworld, from RPM Fusion, provides the actual codecs.
sudo dnf install gstreamer1-plugin-libav libavcodec-freeworld gstreamer1-plugins-good

# Debian / Ubuntu
sudo apt install gstreamer1.0-libav gstreamer1.0-plugins-good
```

On Fedora, `mesa-va-drivers-freeworld` (also from RPM Fusion) additionally
enables hardware decoding on AMD GPUs.

If you install codecs while Wrusp is open, quit it fully and start it again —
"Quit" from the tray icon: closing the window keeps the app alive, relaunching
it only focuses the running instance, and a running process never re-reads
the codecs. If video still won't start after that, delete the GStreamer
registry cache and try once more — GStreamer only rescans when the plugin
file itself changes, not its libraries:

```bash
rm ~/.cache/gstreamer-1.0/registry.*.bin
```

Wrusp ships no codecs: your distribution provides them.

## Building from source

System dependencies (Fedora / Ubuntu names):

```bash
# Fedora
sudo dnf install gcc-c++ webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3 librsvg2-devel

# Ubuntu / Debian
sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev librsvg2-dev
```

Build:

```bash
cd src-tauri
cargo build --release
./target/release/wrusp
```

## Usage

1. Open Wrusp and add an account with a name (e.g. "Personal").
2. WhatsApp Web loads: scan the QR code from your phone
   (WhatsApp → Settings → Linked devices).
3. Repeat for more accounts with the "+" button in the sidebar: each keeps its
   own session and you switch between them with a click.
4. Close the window freely: Wrusp stays in the system tray.

All data lives in `~/.local/share/wrusp/`: `config.json` holds the settings and
`profiles/` the sessions. Deleting an account from the app removes its profile
and signs that session out.

## Architecture (overview)

- `src-tauri/` — Rust backend: single-window shell, accounts, tray, theme and
  persistence.
- `ui/` — settings page (static HTML/CSS/JS, no frameworks).
- A single window holding stacked webviews (one per account plus settings);
  only one is visible at a time. The sidebar is injected into the visible view
  so it behaves identically across profiles.
- Each account uses its own `data_directory` → isolated, persistent webview
  context → independent WhatsApp sessions.
- WhatsApp views have **no** Tauri IPC: they talk to Rust through a custom
  scheme that only accepts a handful of known commands.
- Wrusp does **not** talk to any WhatsApp API: everything happens inside
  WhatsApp Web, just like in a browser.

## Contributing

PRs go through CI (fmt, check, test, blocking clippy and cargo-deny). Before
opening one:

```bash
cd src-tauri
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

## License

[Apache-2.0](LICENSE)

## Known limitations

- **Calls**: on Fedora, WebKitGTK is built without WebRTC, so
  `RTCPeerConnection` doesn't exist and WhatsApp reports that the browser
  doesn't support calls. Wrusp cannot enable it: it depends on the
  distribution.
- **Video**: depends on the GStreamer plugins installed on your system (see
  above).
- On GNOME you need the
  [AppIndicator](https://extensions.gnome.org/extension/615/appindicator-support/)
  extension for the tray icon.
