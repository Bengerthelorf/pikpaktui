# pikpaktui

A TUI and CLI client for [PikPak](https://mypikpak.com) cloud storage, written in pure Rust with no external runtime dependencies.

![pikpaktui screenshot](assets/screenshot.png)

## Features

- **TUI file browser** - Navigate folders, breadcrumb path display, Nerd Font icons
- **CLI subcommands** - `ls` / `mv` / `cp` / `rename` / `rm` / `mkdir` / `download` / `quota`
- **File operations** - Move, copy, rename, delete (trash), create folder
- **Folder picker** - Visual two-pane picker for move/copy destinations, with tab-completion text input as alternative
- **File download** - Download files with resume support
- **Quota query** - Check storage usage
- **Login** - TUI login form with auto-saved credentials and persistent sessions
- **Pure Rust** - Built with `ratatui` + `crossterm` + `reqwest` (rustls), no OpenSSL or C dependencies

## Install

### Homebrew (macOS / Linux)

```bash
brew install Bengerthelorf/tap/pikpaktui
```

### Cargo

```bash
cargo install pikpaktui
```

### From source

```bash
git clone https://github.com/Bengerthelorf/pikpaktui.git
cd pikpaktui
cargo build --release
./target/release/pikpaktui
```

### GitHub Releases

Pre-built binaries for Linux (x86_64, static musl), macOS Intel, and macOS Apple Silicon are available on the [Releases](https://github.com/Bengerthelorf/pikpaktui/releases) page.

## Usage

### TUI mode

Run without arguments to launch the interactive file browser:

```bash
pikpaktui
```

If no valid session exists, a login form will appear. After login, credentials are saved to `config.yaml` and the session is persisted to `session.json`.

### CLI mode

```bash
pikpaktui ls /                                        # List root directory
pikpaktui ls "/My Pack"                               # List a folder
pikpaktui mv "/My Pack/file.txt" /Archive             # Move file
pikpaktui cp "/My Pack/file.txt" /Backup              # Copy file
pikpaktui rename "/My Pack/old.txt" new.txt           # Rename
pikpaktui rm "/My Pack/file.txt"                      # Delete (to trash)
pikpaktui mkdir "/My Pack" newfolder                  # Create folder
pikpaktui download "/My Pack/file.txt"                # Download to current dir
pikpaktui download "/My Pack/file.txt" /tmp/file.txt  # Download to path
pikpaktui quota                                       # Show storage quota
```

CLI mode requires login: it checks for a valid session first, then falls back to `config.yaml` credentials. If neither exists, run `pikpaktui` (TUI) to login.

## TUI Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Open folder |
| `Backspace` | Go back |
| `r` | Refresh |
| `c` | Copy |
| `m` | Move |
| `n` | Rename |
| `d` | Remove (trash) |
| `f` | New folder |
| `h` | Help panel |
| `q` | Quit |

### Folder Picker (Move/Copy)

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` | Open folder |
| `Backspace` | Go back |
| `Space` | Confirm destination |
| `/` | Switch to text input |
| `h` | Help panel |
| `Esc` | Cancel |

### Text Input (Move/Copy)

| Key | Action |
|-----|--------|
| `Tab` | Autocomplete path |
| `Enter` | Select candidate / confirm |
| `Ctrl+B` | Switch to picker |
| `Esc` | Close candidates / cancel |

## Configuration

### Credentials (`config.yaml`)

```
~/.config/pikpaktui/config.yaml
```

```yaml
username: "you@example.com"
password: "your-password"
```

### TUI settings (`config.toml`)

```
~/.config/pikpaktui/config.toml
```

```toml
nerd_font = false       # Enable Nerd Font icons
show_hidden = false     # Show hidden files
move_mode = "picker"    # "picker" (two-pane) or "input" (text input)
show_help_bar = true    # Show help bar at the bottom
```

## Project Structure

```
src/
  main.rs           Entry point, CLI subcommand dispatch
  config.rs         config.yaml / config.toml loading
  pikpak.rs         PikPak API client (auth, file ops, download)
  theme.rs          File icons and colors
  tui/
    mod.rs          App state and event loop
    draw.rs         UI rendering (file list, picker, help sheet)
    handler.rs      Keyboard input handling
    completion.rs   Path tab-completion
```

## License

[Apache-2.0](LICENSE)
