---
name: pikpak
description: Manage PikPak cloud storage — browse, upload, download, stream, share, and organize files via CLI.
version: 0.0.1
metadata:
  openclaw:
    requires:
      bins:
        - pikpaktui
    emoji: "☁️"
    homepage: https://app.snaix.homes/pikpaktui/
---

# PikPak Cloud Storage

You can manage PikPak cloud storage using the `pikpaktui` CLI. This skill covers file management, transfers, sharing, and cloud downloads.

## Authentication

The user must be logged in before any command works. If a command fails with an auth error, ask the user to log in first:

```bash
pikpaktui login -u user@example.com -p password
```

Environment variables `PIKPAK_USER` and `PIKPAK_PASS` are also supported.

## File Management

### List files

```bash
pikpaktui ls /                        # List root
pikpaktui ls -l /Movies               # Long format (id, size, date)
pikpaktui ls --tree --depth=2 /       # Tree view
pikpaktui ls -J /Movies               # JSON output (for parsing)
pikpaktui ls -s size -r /Movies       # Sort by size, reversed
```

Sort fields: `name`, `size`, `created`, `type`, `extension`, `none`.

### Get file info

```bash
pikpaktui info /movie.mkv             # Human-readable
pikpaktui info -J /movie.mkv          # JSON output
```

### Move / Copy / Rename

```bash
pikpaktui mv /file.txt /Archive/
pikpaktui mv -t /Dest /a.txt /b.txt   # Batch move
pikpaktui cp /file.txt /Backup/
pikpaktui rename /old.txt new.txt
```

All support `-n` / `--dry-run` to preview without executing.

### Create folder

```bash
pikpaktui mkdir / NewFolder
pikpaktui mkdir -p / path/to/deep/folder   # Create intermediate dirs
```

### Delete

```bash
pikpaktui rm /file.txt                # Move to trash
pikpaktui rm -rf /old-folder          # Permanent delete, recursive
```

### Trash

```bash
pikpaktui trash                       # List trashed files
pikpaktui untrash file.txt            # Restore from trash
```

### Star / Unstar

```bash
pikpaktui star /movie.mkv /photo.jpg
pikpaktui unstar /movie.mkv
pikpaktui starred                     # List starred files
```

## Downloads & Uploads

### Download files

```bash
pikpaktui download /movie.mkv                  # Download to current dir
pikpaktui download -o output.mkv /movie.mkv    # Custom output name
pikpaktui download -j4 -t ./local /Movies      # Concurrent, to local dir
```

### Upload files

```bash
pikpaktui upload file.txt                      # Upload to root
pikpaktui upload -t /Remote a.txt b.txt        # Batch upload to folder
```

### Get direct download URL

```bash
pikpaktui link /movie.mkv             # Print download URL
pikpaktui link -mc /movie.mkv         # Media streams + copy to clipboard
pikpaktui link -J /movie.mkv          # JSON output
```

## Cloud (Offline) Downloads

Submit URLs or magnet links for server-side downloading:

```bash
pikpaktui offline https://example.com/file.zip
pikpaktui offline --to /Downloads magnet:?xt=...
```

### Manage tasks

```bash
pikpaktui tasks                       # List tasks
pikpaktui tasks list 10               # Limit to 10
pikpaktui tasks retry <task_id>       # Retry failed task
pikpaktui tasks delete <task_id>      # Delete task
pikpaktui tasks -J                    # JSON output
```

## Sharing

### Create share link

```bash
pikpaktui share /movie.mkv                    # Basic share
pikpaktui share -p -d 7 /folder               # Password + 7-day expiry
pikpaktui share -J /file.txt                  # JSON output
```

### List / Save / Delete shares

```bash
pikpaktui share -l                             # List your shares
pikpaktui share -S https://mypikpak.com/s/abc  # Save share to drive
pikpaktui share -S <url> -p <code> -t /Dest    # With password + dest
pikpaktui share -D <share_id>                  # Delete share
```

## Video Playback

Stream videos to a local player (requires `player` configured in `~/.config/pikpaktui/config.toml`):

```bash
pikpaktui play /movie.mkv              # Original quality
pikpaktui play /movie.mkv 1080         # Specific quality
```

## Account Info

```bash
pikpaktui quota                        # Storage usage
pikpaktui quota -J                     # JSON output
pikpaktui vip                          # VIP membership info
pikpaktui events                       # Recent file events
```

## Tips

- Use `-J` / `--json` on most commands for machine-parseable output.
- Use `-n` / `--dry-run` to preview destructive or batch operations.
- All path arguments are cloud paths (e.g. `/Movies/file.mkv`), not local paths.
- `pikpaktui update` self-updates the binary to the latest release.
- Preview text files with `pikpaktui cat /notes.txt`.
