---
title: Getting Started
section: guide
order: 1
---

pikpaktui is a terminal client for [PikPak](https://mypikpak.com) cloud
storage — an interactive TUI plus a full CLI with 28 subcommands. Pure
Rust, no OpenSSL, no C deps.

If you don't have it yet, head to [**install**](/install) first. This
page assumes `pikpaktui` is on your `$PATH`.

## First Launch — TUI

Run with no arguments to open the interactive TUI:

```bash
pikpaktui
```

On first run, a login form appears. Enter your PikPak email and password.
Credentials are saved to `~/.config/pikpaktui/login.toml` and the session
to `~/.config/pikpaktui/session.json`.

![TUI main view](/images/main.jpeg)

:::callout[Keys to know]{kind="info"}
- `,` — open Settings
- `h` — show the full help sheet
- `q` — quit
:::

See the [TUI Guide](/guide/tui) for the full keybinding reference.

## CLI Quick Start

```bash
# List
pikpaktui ls /
pikpaktui ls -l "/My Pack"          # long format
pikpaktui ls --tree --depth=2 /     # recursive tree

# File ops
pikpaktui mv "/My Pack/file.txt" /Archive
pikpaktui cp "/My Pack/video.mp4" /Backup
pikpaktui rename "/My Pack/old.txt" new.txt
pikpaktui rm "/My Pack/file.txt"            # to trash
pikpaktui rm -rf "/My Pack/old-folder"      # permanent

# Transfer
pikpaktui download "/My Pack/video.mp4"
pikpaktui download -j 4 "/My Pack/Movies"   # concurrent files within a folder
pikpaktui upload ./notes.txt "/My Pack"

# Offline
pikpaktui offline "magnet:?xt=urn:btih:..."
pikpaktui offline --to "/Downloads" "https://example.com/file.zip"
```

:::callout[Dry run]{kind="info"}
Most file and transfer commands accept `-n` / `--dry-run` — check the
individual command reference before relying on it.
:::

## Next Steps

- [TUI Guide](/guide/tui) — every keybinding for every view
- [Configuration](/guide/configuration) — colors, fonts, player, behavior
- [CLI Commands](/docs/cli/commands) — full reference for all 28 subcommands
- [Shell Completions](/guide/shell-completions) — tab-complete cloud paths
