---
title: CLI Overview
section: cli
order: 1
---


pikpaktui provides 27 CLI subcommands for scripting, automation, and power-user workflows. All commands require a valid session — run `pikpaktui` (TUI) first to log in, or use `pikpaktui login`.

## Command Groups

### File Management

| Command | Description |
|---------|-------------|
| [`ls`](/cli/commands#ls) | List files and folders |
| [`mv`](/cli/commands#mv) | Move files or folders |
| [`cp`](/cli/commands#cp) | Copy files or folders |
| [`rename`](/cli/commands#rename) | Rename a file or folder |
| [`rm`](/cli/commands#rm) | Remove to trash (or permanently with `-f`) |
| [`mkdir`](/cli/commands#mkdir) | Create folders |
| [`info`](/cli/commands#info) | Detailed file/folder metadata |
| [`link`](/cli/commands#link) | Get direct download URL |
| [`cat`](/cli/commands#cat) | Preview text file contents |

### Playback

| Command | Description |
|---------|-------------|
| [`play`](/cli/commands#play) | Stream video with external player |

### Transfer

| Command | Description |
|---------|-------------|
| [`download`](/cli/commands#download) | Download files or folders |
| [`upload`](/cli/commands#upload) | Upload files to PikPak |
| [`share`](/cli/commands#share) | Create, list, save, or delete share links |

### Cloud Download

| Command | Description |
|---------|-------------|
| [`offline`](/cli/commands#offline) | Submit URL or magnet for server-side download |
| [`tasks`](/cli/commands#tasks) | Manage offline download tasks |

### Trash

| Command | Description |
|---------|-------------|
| [`trash`](/cli/commands#trash) | List trashed files |
| [`untrash`](/cli/commands#untrash) | Restore files from trash by name |

### Starred & Activity

| Command | Description |
|---------|-------------|
| [`star`](/cli/commands#star) | Star files |
| [`unstar`](/cli/commands#unstar) | Unstar files |
| [`starred`](/cli/commands#starred) | List starred files |
| [`events`](/cli/commands#events) | Recent file activity |

### Auth

| Command | Description |
|---------|-------------|
| [`login`](/cli/commands#login) | Log in and save credentials |

### Account

| Command | Description |
|---------|-------------|
| [`quota`](/cli/commands#quota) | Storage and bandwidth quota |
| [`vip`](/cli/commands#vip) | VIP status and account info |

### Utility

| Command | Description |
|---------|-------------|
| [`update`](/cli/commands#update) | Check for updates and self-update |
| [`completions`](/cli/commands#completions) | Generate shell completions |

## Common Flags

### JSON output

Most commands that list data support `-J` / `--json` for machine-readable output — pipe to `jq` for filtering:

```bash
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'
pikpaktui info "/My Pack/video.mp4" --json
pikpaktui quota --json
```

### Dry run

All commands that modify data accept `-n` / `--dry-run`. This resolves paths and prints a detailed plan without making any changes:

```bash
pikpaktui rm -n "/My Pack/file.txt"
pikpaktui mv -n "/My Pack/a.txt" /Archive
pikpaktui download -n "/My Pack/folder"
pikpaktui upload -n ./file.txt "/My Pack"
```

### Batch mode (`-t`)

`mv`, `cp`, `download`, and `upload` support `-t <destination>` for operating on multiple items at once:

```bash
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt
pikpaktui download -t ./local/ /a.mp4 /b.mp4
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt
```
