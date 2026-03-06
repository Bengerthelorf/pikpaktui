# Configuration

All configuration files live under `~/.config/pikpaktui/`.

## Credentials — `login.yaml`

```yaml
username: "you@example.com"
password: "your-password"
```

## TUI Settings — `config.toml`

```toml
[tui]
nerd_font = false         # Nerd Font icons in TUI
cli_nerd_font = false     # Nerd Font icons in CLI output
move_mode = "picker"      # "picker" (two-pane) or "input" (text input)
show_help_bar = true      # Bottom help bar
border_style = "thick"    # "rounded" | "thick" | "thick-rounded" | "double"
color_scheme = "vibrant"  # "vibrant" | "classic" | "custom"
show_preview = true       # Three-column layout (false = two-column)
lazy_preview = false      # Auto-load preview on cursor move
preview_max_size = 65536  # Max bytes for text preview (default 64 KB)
thumbnail_mode = "auto"   # "auto" | "off" | "force-color" | "force-grayscale"
sort_field = "name"       # "name" | "size" | "created" | "type" | "extension" | "none"
sort_reverse = false      # Reverse sort direction
player = "mpv"            # External video player command (mpv, vlc, iina, etc.)

# Per-terminal image protocol configuration
# Detected via $TERM_PROGRAM environment variable
[tui.image_protocols]
ghostty = "kitty"
"iTerm.app" = "iterm2"
WezTerm = "auto"          # "auto" | "kitty" | "iterm2" | "sixel"

# Custom colors (only used when color_scheme = "custom")
[tui.custom_colors]
folder = [92, 176, 255]
archive = [255, 102, 102]
image = [255, 102, 255]
video = [102, 255, 255]
audio = [0, 255, 255]
document = [102, 255, 102]
code = [255, 255, 102]
default = [255, 255, 255]
```

## Session — `session.json`

Auto-managed. Stores access/refresh tokens. No manual editing needed.

## Download State — `downloads.json`

Auto-managed. Persists incomplete download tasks (pending / paused / failed) across sessions.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PIKPAK_DRIVE_BASE_URL` | Override PikPak drive API endpoint |
| `PIKPAK_AUTH_BASE_URL` | Override PikPak auth API endpoint |
| `PIKPAK_CLIENT_ID` | Override OAuth client ID |
| `PIKPAK_CLIENT_SECRET` | Override OAuth client secret |
| `PIKPAK_CAPTCHA_TOKEN` | Provide CAPTCHA token for login |
