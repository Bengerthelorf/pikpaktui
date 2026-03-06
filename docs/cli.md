# CLI Reference

CLI mode requires a valid session. If no session exists, run `pikpaktui` (TUI) first to log in.

## File Management

```bash
pikpaktui ls /                                        # Colored multi-column grid
pikpaktui ls -l "/My Pack"                            # Long format (id + size + date + name)
pikpaktui ls --sort=size -r /                         # Sort by size, largest last
pikpaktui ls -s created "/My Pack"                    # Sort by creation time, newest first
pikpaktui ls --tree /                                 # Recursive tree view
pikpaktui ls --tree --depth=2 "/My Pack"              # Tree limited to 2 levels deep
pikpaktui ls --tree -l /Movies                        # Tree with size and date columns
pikpaktui ls /Movies --json                           # JSON output (pipe to jq)
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'

pikpaktui mv "/My Pack/file.txt" /Archive             # Move file
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt        # Batch move to target
pikpaktui cp "/My Pack/file.txt" /Backup              # Copy file
pikpaktui cp -t /Backup /a.txt /b.txt                 # Batch copy to target
pikpaktui rename "/My Pack/old.txt" new.txt           # Rename
pikpaktui rm "/My Pack/file.txt"                      # Delete file (to trash)
pikpaktui rm /a.txt /b.txt /c.txt                     # Batch delete (to trash)
pikpaktui rm -r "/My Pack/folder"                     # Delete folder (to trash)
pikpaktui rm -rf "/My Pack/folder"                    # Delete folder permanently
pikpaktui mkdir "/My Pack" newfolder                  # Create folder
pikpaktui mkdir -p "/My Pack/a/b/c"                   # Create nested folders recursively
```

## Viewing & Info

```bash
pikpaktui info "/My Pack/video.mp4"                   # Detailed file info (media metadata)
pikpaktui info "/My Pack/video.mp4" --json            # JSON (includes hash, links, media tracks)
pikpaktui link "/My Pack/file.zip"                    # Print direct download URL
pikpaktui link "/My Pack/file.zip" --copy             # Copy URL to clipboard
pikpaktui link "/My Pack/video.mp4" -m                # Also print video streaming URLs
pikpaktui link "/My Pack/file.zip" --json             # JSON output {name, url, size}
pikpaktui cat "/My Pack/notes.txt"                    # Preview text file contents
```

## Video Playback

```bash
pikpaktui play "/My Pack/video.mp4"                   # List available streams (720p, 1080p, etc.)
pikpaktui play "/My Pack/video.mp4" 1080p             # Play 1080p stream with configured player
pikpaktui play "/My Pack/video.mp4" original          # Play original quality
pikpaktui play "/My Pack/video.mp4" 2                 # Play by stream number
```

## Transfers

```bash
pikpaktui download "/My Pack/file.txt"                # Download to current dir
pikpaktui download "/My Pack/file.txt" /tmp/file.txt  # Download to specific path
pikpaktui download -o output.mp4 "/My Pack/video.mp4" # Download with custom output name
pikpaktui download "/My Pack/folder"                  # Download entire folder recursively
pikpaktui download -t ./videos/ /a.mp4 /b.mp4         # Batch download to directory
pikpaktui upload ./local-file.txt "/My Pack"          # Upload (dedup + resumable)
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt        # Batch upload to target
```

## Sharing

```bash
pikpaktui share "/My Pack/file.txt"                   # Create share link
pikpaktui share -p "/My Pack/file.txt"                # Encrypted share (auto-generates password)
pikpaktui share -d 7 "/My Pack/file.txt"              # Share that expires in 7 days
pikpaktui share -J "/My Pack/file.txt"                # JSON output {share_id, share_url, pass_code}
pikpaktui share -l                                    # List your shares (id, title, views, saves)
pikpaktui share -l -J                                 # JSON output
pikpaktui share -D <share_id>                         # Delete a share
pikpaktui share -S "https://mypikpak.com/s/XXXX"     # Save shared link to your drive
pikpaktui share -S -p PO -t "/My Pack" "https://mypikpak.com/s/XXXX"  # With password and destination
```

## Offline Download & Tasks

```bash
pikpaktui offline "magnet:?xt=..."                    # Submit magnet link
pikpaktui offline "https://example.com/file.zip" --to "/Downloads" --name "file.zip"
pikpaktui tasks                                       # List offline tasks
pikpaktui tasks list --json                           # JSON output
pikpaktui tasks retry <task_id>                       # Retry failed task
pikpaktui tasks rm <task_id>                          # Delete task
```

## Star, Trash & Account

```bash
pikpaktui star "/My Pack/file.txt"                    # Star a file
pikpaktui unstar "/My Pack/file.txt"                  # Unstar
pikpaktui starred                                     # List starred files
pikpaktui starred -l                                  # Long format
pikpaktui starred --json                              # JSON output
pikpaktui events                                      # Recent file events
pikpaktui events --json                               # JSON output

pikpaktui trash                                       # List trashed files
pikpaktui trash -l                                    # Long format
pikpaktui trash --json                                # JSON output
pikpaktui untrash "file.txt"                          # Restore from trash

pikpaktui quota                                       # Storage and bandwidth quota
pikpaktui quota --json                                # JSON output
pikpaktui vip                                         # VIP status, invite code, transfer quota
```

## Dry Run

All mutating commands support `-n` / `--dry-run` — resolves paths and prints a plan without making any changes.

```bash
pikpaktui rm -n "/My Pack/file.txt"                   # Show what would be trashed
pikpaktui rm -n -rf "/My Pack/folder"                 # Show what would be permanently deleted
pikpaktui mv -n "/My Pack/a.txt" /Archive             # Show move plan
pikpaktui cp -n -t /Backup /a.txt /b.txt              # Show batch copy plan
pikpaktui rename -n "/My Pack/old.txt" new.txt        # Show rename plan
pikpaktui mkdir -n -p "/My Pack/a/b/c"                # Show which folders would be created
pikpaktui download -n "/My Pack/folder"               # Show what would be downloaded
pikpaktui upload -n ./file.txt "/My Pack"             # Show upload plan
pikpaktui share -S -n "https://mypikpak.com/s/XXXX"  # Show items that would be saved
pikpaktui offline "magnet:?xt=..." --dry-run --to "/Downloads"  # Show task that would be submitted
```
