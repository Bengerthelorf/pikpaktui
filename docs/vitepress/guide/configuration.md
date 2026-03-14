# Configuration

All configuration files live under `~/.config/pikpaktui/`.

## Credentials — `login.yaml`

Stores your PikPak account credentials. Created automatically on first login (via TUI or `pikpaktui login`).

```yaml
username: "you@example.com"
password: "your-password"
```

You can also set credentials via environment variables for the `login` command:

```bash
PIKPAK_USER=you@example.com PIKPAK_PASS=yourpassword pikpaktui login
```

::: warning
Credentials are stored in plain text. Ensure `~/.config/pikpaktui/` has appropriate permissions (`chmod 700`).
:::

## TUI & CLI Settings — `config.toml`

The main settings file. Edit manually or use the in-TUI settings panel (`,` to open, `s` to save).

```toml
[tui]
# UI
nerd_font = false           # Nerd Font icons in TUI (requires a Nerd Font terminal)
border_style = "thick"      # "rounded" | "thick" | "thick-rounded" | "double"
color_scheme = "vibrant"    # "vibrant" | "classic" | "custom"
show_help_bar = true        # Bottom keybinding hint bar
quota_bar_style = "bar"     # "bar" (visual bar) | "percent" (numeric %)

# Preview
show_preview = true         # Three-column layout; false = two-column
lazy_preview = false        # Only load preview when cursor stops moving
preview_max_size = 65536    # Max bytes loaded for text preview (default: 64 KB)
thumbnail_mode = "auto"     # "auto" | "off" | "force-color" | "force-grayscale"
thumbnail_size = "medium"   # "small" | "medium" | "large"

# Sort (persisted when changed with S / R in TUI)
sort_field = "name"         # "name" | "size" | "created" | "type" | "extension" | "none"
sort_reverse = false

# Interface
move_mode = "picker"        # "picker" (two-pane GUI) | "input" (text input with tab-completion)
cli_nerd_font = false       # Nerd Font icons in CLI output

# Playback
player = "mpv"              # External video player command; set in TUI on first video play

# Downloads
download_jobs = 1           # Concurrent download threads (1–16)
update_check = "notify"     # "notify" | "quiet" | "off"
```

### update_check

Controls update checking behavior.

- `"notify"` (default) — Check for updates on startup; show persistently in TUI status bar and CLI stderr
- `"quiet"` — Check silently; only show in TUI log
- `"off"` — Disable update checking entirely

```toml
update_check = "notify"
```

### Image Protocols

Configure the image rendering protocol per terminal emulator, keyed by the `$TERM_PROGRAM` environment variable. Detected automatically — entries are added the first time each terminal is used.

```toml
[tui.image_protocols]
ghostty = "kitty"
"iTerm.app" = "iterm2"
WezTerm = "auto"
```

Supported values: `"auto"` (detect), `"kitty"`, `"iterm2"`, `"sixel"`.

### Custom Colors

Used when `color_scheme = "custom"`. Each value is an `[R, G, B]` array (0–255).

```toml
[tui.custom_colors]
folder   = [92, 176, 255]   # Light blue
archive  = [255, 102, 102]  # Light red
image    = [255, 102, 255]  # Light magenta
video    = [102, 255, 255]  # Light cyan
audio    = [0, 255, 255]    # Cyan
document = [102, 255, 102]  # Light green
code     = [255, 255, 102]  # Light yellow
default  = [255, 255, 255]  # White
```

You can edit custom colors in the TUI: open Settings (`,`), select **Color Scheme**, press `Enter` to enter the custom color editor, then use `r` / `g` / `b` to edit each RGB component.

## Auto-managed Files

These are maintained automatically. Do not edit manually.

| File | Description |
|------|-------------|
| `session.json` | Access and refresh tokens (auto-refreshed) |
| `downloads.json` | Incomplete download state — survives restarts |

## Environment Variables

These override config file values. Useful for CI or per-session overrides.

| Variable | Description |
|----------|-------------|
| `PIKPAK_USER` | Account email (for `pikpaktui login`) |
| `PIKPAK_PASS` | Account password (for `pikpaktui login`) |
| `PIKPAK_DRIVE_BASE_URL` | Override PikPak drive API endpoint |
| `PIKPAK_AUTH_BASE_URL` | Override PikPak auth API endpoint |
| `PIKPAK_CLIENT_ID` | Override OAuth client ID |
| `PIKPAK_CLIENT_SECRET` | Override OAuth client secret |
| `PIKPAK_CAPTCHA_TOKEN` | CAPTCHA token if login is challenged |

::: tip Concurrent downloads
Set `download_jobs` to match your bandwidth. Values between 2–4 are typical. Maximum is 16.
:::
