---
title: Configuration
section: guide
order: 3
---


All configuration files live under `~/.config/pikpaktui/`.

## Credentials — `login.toml`

Stores your PikPak account credentials. Created automatically on first login (via TUI or `pikpaktui login`).

```toml
username = "you@example.com"
password = "your-password"
```

You can also set credentials via environment variables for the `login` command:

```bash
PIKPAK_USER=you@example.com PIKPAK_PASS=yourpassword pikpaktui login
```

:::callout[warning]{kind="warn"}
Credentials are stored in plain text. Ensure `~/.config/pikpaktui/` has appropriate permissions (`chmod 700`).
:::

## TUI & CLI Settings — `config.toml`

The main settings file. Edit manually or use the in-TUI settings panel (`,` to open, `s` to save).
The scalar settings are top-level TOML keys; do not wrap them in a `[tui]` table.

```toml
# UI
nerd_font = false           # Nerd Font icons in TUI (requires a Nerd Font terminal)
border_style = "thick"      # "rounded" | "thick" | "thick-rounded" | "double"
color_scheme = "vibrant"    # "vibrant" | "classic" | "custom"
show_help_bar = true        # Bottom keybinding hint bar
quota_bar_style = "bar"     # "bar" (visual bar) | "percent" (numeric %)

# Browser
columns = "auto"           # "auto" or a positive integer (4, 5, 6, ...)
column_min_width = 28       # Minimum pane width in terminal cells; at least 16
inline_thumbnails = true   # Tiny image before filename; v toggles
inline_thumbnail_size = "tiny"  # "tiny" | "small" | "medium" | "large"; [ / ] resize
thumbnail_placeholders = true  # Reserve a grey image card while waiting or unavailable

# Preview
show_preview = true         # Include preview when width allows; false = browser panes only
lazy_preview = false        # Auto-load text/file details after cursor stops
preview_max_size = 65536    # Max bytes loaded for text preview (default: 64 KB)
thumbnail_mode = "auto"     # "auto" | "off" | "force-color" | "force-grayscale"
thumbnail_size = "medium"   # Server thumbnail resolution; separate from inline display size; "small" | "medium" | "large"

# Sort (persisted when changed with S / R in TUI)
sort_field = "name"         # "name" | "size" | "created" | "type" | "extension" | "none"
sort_reverse = false

# Interface
move_mode = "picker"        # "picker" (two-pane GUI) | "input" (text input with tab-completion)
cli_nerd_font = false       # Nerd Font icons in CLI output

# Playback
player = "mpv"              # External video player command; set in TUI on first video play
playback_quality = "original" # Preferred TUI playback quality; original | 2160p | 1440p | 1080p | 720p | 480p | 360p

# Downloads
download_jobs = 1           # TUI download workers (1–16 in the settings UI)
update_check = "notify"     # "notify" | "quiet" | "off"
```

The default playback preference is `original`: source quality, not a fixed resolution. Change it in **Playback Settings → Default Playback Quality**, then press `s` to save, or set the top-level `playback_quality` key. `4k` is accepted as an alias for `2160p`. An explicit resolution prefers a permitted transcode: the highest at or below the target, then the lowest above it, then the original/first permitted link when resolution metadata is unavailable. The Play Video dialog shows the actual selection and a notice if the preference is unavailable. Per-file changes in the dropdown do not change the saved default. CLI `play` still lists streams when quality is omitted and uses an explicitly supplied name/index when provided.

### update_check

Controls update checking behavior.

- `"notify"` (default) — Check for updates on startup; show persistently in TUI status bar and CLI stderr
- `"quiet"` — Check silently; only show in TUI log
- `"off"` — Disable update checking entirely

```toml
update_check = "notify"
```

### Thumbnail settings

**Browser Settings** exposes columns, minimum column width, inline thumbnails, inline display size, thumbnail placeholders and thumbnail source size. These scalar keys stay at the top level of `config.toml`.

`inline_thumbnail_size` controls the terminal space occupied by a row image. `thumbnail_size` controls the server image resolution (`small`, `medium`, `large`); saving **Thumbnail Source Size** updates shared workers and refreshes the browser immediately.

`thumbnail_placeholders = true` is the default for both new and older configs. Pending images use a quiet grey card, loading images show a spinner, and unavailable images show a cross. Image and video files without a thumbnail URL also keep an unavailable card. Row height and filename alignment stay stable. Set it to `false` to restore the icon fallback. `inline_thumbnails` controls row images; `show_preview` controls preview panes; `thumbnail_mode` controls image rendering in the large preview.

Playback selection, task completion checks, token recovery and stale-response fixes apply automatically and do not need compatibility switches.

### Image Protocols

Configure the image rendering protocol per terminal emulator, keyed by the `$TERM_PROGRAM` environment variable. Detected automatically — entries are added the first time each terminal is used.

```toml
[image_protocols]
ghostty = "kitty"
"iTerm.app" = "iterm2"
WezTerm = "auto"
```

Supported values: `"auto"` (detect), `"kitty"`, `"iterm2"`, `"sixel"`.

### Custom Colors

Used when `color_scheme = "custom"`. Each value is an `[R, G, B]` array (0–255).

```toml
[custom_colors]
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

:::callout[Concurrent downloads]{kind="info"}
`download_jobs` controls the TUI download queue only; CLI folder downloads use `download -j <n>`. Values between 2–4 are typical, and the TUI settings editor offers 1–16.
:::
