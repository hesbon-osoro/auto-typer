# Auto Typer

Types your text into any window, one key at a time, like a person would. It works on Windows, macOS and Linux.

Paste your text and click **Start typing**. Then click into the box where it should go. Auto Typer types it with a natural rhythm: uneven timing, short pauses after punctuation, and the occasional typo that it fixes. Switch windows, click, or press **Esc**, and it pauses. **Resume** goes back to the same window and carries on from the same letter.

It's useful wherever pasting doesn't work: VM, VNC and RDP consoles, KVMs, kiosks, and some web forms. It also helps with screencasts and demos, and with accessibility setups. Only use it where automated typing is allowed.

> **Status: early bootstrap (v0.1).** The typing engine is tested. The desktop app compiles, but it hasn't been tried on every OS yet. See the [roadmap](#roadmap).

## Features

- **Human rhythm.** Speed drifts over time. Common letter pairs come faster and same-finger reaches come slower. Pauses come after sentences, commas and new lines, and before long words. The WPM setting already includes the pauses, so long texts finish close to the speed you set.
- **Typos that get fixed.** Typos are wrong neighbouring keys, swapped letters or doubled letters. Sometimes one or two more letters go in before it "notices" and backspaces. Set typos to 0% to turn them off.
- **Pause and resume.** Typing pauses on Esc, a mouse click (optional), switching windows, or the Pause button. If it paused in the middle of a typo, it cleans that up before carrying on.
- **Type part of the text.** Highlight a section before Start to type only that part.
- **Remembers the target window.** The next Start goes straight back to it.
- **Code editor mode.** Keeps your text's exact indentation even when the editor auto-indents. It drops brackets the editor closed by itself, and stops autocomplete from swallowing Enter.
- **Shift+Enter for new lines,** for chat apps where Enter sends the message.
- **Any character.** Letters, digits and space are sent as real key presses. Other symbols, accents and emoji are sent as Unicode text, so it doesn't matter which keyboard layout you use.
- **Private.** It makes no network calls. Settings stay on your computer, and "Keep text after closing" can be turned off.

## Platform support

| | Windows 10/11 | macOS 11+ | Linux X11 | Linux Wayland |
| --- | --- | --- | --- | --- |
| Typing | ✅ | ✅ (needs Accessibility permission) | ✅ | ❌ not yet |
| Pause on focus change / click / Esc | ✅ | ✅ (needs Input Monitoring) | ✅ | ❌ |
| Bring target window back | ✅ | ✅ (app level) | ✅ (EWMH window managers) | ❌ |

Wayland deliberately blocks apps from typing into other apps. Support is planned through the XDG RemoteDesktop portal (libei), and enigo already has experimental support for it. Until then, log into an "Ubuntu on Xorg" session.

## Development

Prerequisites: [Rust](https://rustup.rs), Node 20+ and pnpm, plus the webview and X11 development libraries. On Debian/Ubuntu (package set checked against Ubuntu 24.04):

```sh
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev librsvg2-dev libx11-dev
```

This pulls in GTK, D-Bus (`libdbus-1-dev`) and the other libraries Tauri links against. The app has no tray icon, so it does not need an appindicator package. Leave out `libappindicator3-dev` in particular: on Ubuntu 24.04 it conflicts with the Ayatana indicator that the desktop already installs, and apt then refuses the whole install. For other distros, see the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

Then:

```sh
pnpm install
pnpm tauri dev        # run the app
pnpm test             # typing engine tests (no display needed)
pnpm tauri build      # installer for this OS
```

### How the code is organised

| Path | What's in it |
| --- | --- |
| `crates/typing-plan/` | The engine. It turns text and settings into a stream of actions (`Wait`, `Char`, `Scratch`, `Erase`, `Key`, `Progress`): rhythm, pauses, typos, code-mode key sequences. It's pure Rust with no OS calls, and it's unit-tested. |
| `src-tauri/src/typist.rs` | The worker thread. It sends the actions as real key presses with [enigo](https://crates.io/crates/enigo), polls Esc, clicks and held modifiers with [device_query](https://crates.io/crates/device_query), pauses and resumes, and reports progress to the UI. |
| `src-tauri/src/focus.rs` | Reads which window has focus and brings a window back to the front, with one implementation each for Win32, X11 and AppKit. |
| `src-tauri/src/lib.rs` | The Tauri commands: `start_typing`, `pause_typing`, `resume_typing`, `stop_typing`, `update_typing`. |
| `src/` | The React UI: text box, live highlight of what's been typed, speed and typo sliders, options. |

Text positions are Unicode scalar indices after normalising line endings to `\n`, on both sides (`chars()` in Rust, `Array.from()` in JS).

## Releases

Installers are built by GitHub Actions from a tagged commit (`git tag v0.1.0 && git push --tags`) and attached to a draft release. Binaries are never committed to the repository, so every download can be traced back to the source that built it.

## Roadmap

- [ ] Try it out by hand on Windows, macOS and X11, and fix whatever turns up
- [ ] Remote PC mode for Windows: send scan codes with careful key timing and Alt-code fallback for symbols, so RDP, Parsec and AnyDesk don't drop or repeat keys
- [ ] Wayland support through the RemoteDesktop portal (libei)
- [ ] Don't type into the desktop, taskbar or Start menu when they're clicked as the "target"
- [ ] macOS: guide the user through the Accessibility and Input Monitoring prompts
- [ ] Typo neighbours for non-QWERTY layouts
- [ ] App icon and signed installers (Windows code signing, macOS notarisation)

## License

[MIT](LICENSE)
