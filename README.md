# pikpaktui

A terminal-based client for [PikPak](https://mypikpak.com) cloud storage — browse, manage, and download your files without leaving the terminal. Written in pure Rust, no external runtime dependencies.

| ![main](assets/main.jpeg) | ![settings](assets/settings.png) | ![help](assets/help.png) |
| --- | --- | --- |
| ![cart](assets/cart.png) | ![downloads](assets/downloads.jpeg) | ![downloads](assets/downloads_mian.png) |
| ![copy](assets/copy.png) | ![trash](assets/trash.png) | ![play](assets/play.png) |

## What It Does

**Interactive TUI** — A three-column Miller layout (like Yazi) that lets you navigate your PikPak drive visually. Preview thumbnails, syntax-highlighted code, and folder contents right in the terminal. Move, copy, rename, delete, star files, stream videos, manage offline downloads — all from the keyboard.

**Full CLI** — 26 subcommands (`ls`, `mv`, `cp`, `rm`, `download`, `upload`, `share`, and more) with colored output, JSON mode for scripting, and dry-run support so you can preview changes before committing them.

**Pure Rust** — Built on `ratatui` + `crossterm` + `reqwest` (rustls-tls). No OpenSSL, no C dependencies. Runs on Linux (x86_64 musl static), macOS Intel, and macOS Apple Silicon.

## Install

### Homebrew (macOS / Linux)

```bash
brew install Bengerthelorf/tap/pikpaktui
```

### Cargo

```bash
cargo install pikpaktui
```

### From Source

```bash
git clone https://github.com/Bengerthelorf/pikpaktui.git
cd pikpaktui
cargo build --release
./target/release/pikpaktui
```

Pre-built binaries are also available on the [Releases](https://github.com/Bengerthelorf/pikpaktui/releases) page.

## Quick Start

Launch the TUI with no arguments. On first run a login form will appear — after that your session is saved and auto-refreshes.

```bash
pikpaktui
```

For CLI usage, add a subcommand:

```bash
pikpaktui ls /
pikpaktui download "/My Pack/video.mp4"
pikpaktui upload ./local.txt "/My Pack"
```

Press `,` for settings, `h` for help, `q` to quit.

For more details, see the docs:

- [CLI Reference](docs/cli.md) — All 26 subcommands with examples
- [TUI Guide](docs/tui.md) — Keybindings for every view
- [Configuration](docs/configuration.md) — Config files, settings, environment variables
- [Shell Completions](docs/shell-completions.md) — Zsh setup with dynamic cloud path completion
- [Project Structure](docs/project-structure.md) — Source code layout

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PIKPAK_DRIVE_BASE_URL` | Override PikPak drive API endpoint |
| `PIKPAK_AUTH_BASE_URL` | Override PikPak auth API endpoint |
| `PIKPAK_CLIENT_ID` | Override OAuth client ID |
| `PIKPAK_CLIENT_SECRET` | Override OAuth client secret |
| `PIKPAK_CAPTCHA_TOKEN` | Provide CAPTCHA token for login |

## License

[Apache-2.0](LICENSE)
