# Command Reference

All commands require a valid session. Run `pikpaktui` (TUI) to log in first, or use [`login`](#login).

---

## ls

List files and folders in your PikPak drive.

```
pikpaktui ls [options] [path]
```

| Flag | Description |
|------|-------------|
| `-l`, `--long` | Long format — shows ID, size, date, and name |
| `-J`, `--json` | Output as JSON array |
| `-s`, `--sort <field>` | Sort by: `name`, `size`, `created`, `type`, `extension`, `none` |
| `-r`, `--reverse` | Reverse sort order |
| `--tree` | Recursive tree view |
| `--depth=N` | Limit tree depth to N levels |

**Examples:**

```bash
pikpaktui ls                               # list root (/)
pikpaktui ls "/My Pack"                   # list a folder
pikpaktui ls -l /Movies                   # long format
pikpaktui ls --sort=size -r /             # sort by size, largest first
pikpaktui ls -s created "/My Pack"        # sort by creation time
pikpaktui ls --tree /                     # full recursive tree
pikpaktui ls --tree --depth=2 "/My Pack"  # tree, max 2 levels
pikpaktui ls --tree -l /Movies            # tree with sizes and dates
pikpaktui ls /Movies --json               # JSON output
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'
```

---

## mv

Move files or folders to a destination folder.

```
pikpaktui mv [options] <src> <dst>
pikpaktui mv [options] -t <dst> <src...>
```

| Flag | Description |
|------|-------------|
| `-t <dst>` | Batch mode — move multiple sources into `<dst>` |
| `-n`, `--dry-run` | Preview without executing |

**Examples:**

```bash
pikpaktui mv "/My Pack/file.txt" /Archive
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt   # batch
pikpaktui mv -n "/My Pack/a.txt" /Archive        # dry run
```

---

## cp

Copy files or folders to a destination folder.

```
pikpaktui cp [options] <src> <dst>
pikpaktui cp [options] -t <dst> <src...>
```

| Flag | Description |
|------|-------------|
| `-t <dst>` | Batch mode — copy multiple sources into `<dst>` |
| `-n`, `--dry-run` | Preview without executing |

**Examples:**

```bash
pikpaktui cp "/My Pack/file.txt" /Backup
pikpaktui cp -t /Backup /a.txt /b.txt            # batch
pikpaktui cp -n -t /Backup /a.txt /b.txt         # dry run
```

---

## rename

Rename a file or folder in place (stays in its current directory).

```
pikpaktui rename [options] <path> <new_name>
```

| Flag | Description |
|------|-------------|
| `-n`, `--dry-run` | Preview without executing |

**Examples:**

```bash
pikpaktui rename "/My Pack/old.txt" new.txt
pikpaktui rename -n "/My Pack/old.txt" new.txt   # dry run
```

---

## rm

Remove files or folders. By default, moves to trash (recoverable). Use `-f` for permanent deletion.

```
pikpaktui rm [options] <path...>
```

| Flag | Description |
|------|-------------|
| `-r`, `--recursive` | Required to remove folders |
| `-f`, `--force` | Permanently delete (bypass trash) |
| `-rf`, `-fr` | Remove folder permanently |
| `-n`, `--dry-run` | Preview without executing |

**Examples:**

```bash
pikpaktui rm "/My Pack/file.txt"             # move to trash
pikpaktui rm /a.txt /b.txt /c.txt            # batch trash
pikpaktui rm -r "/My Pack/folder"            # folder to trash
pikpaktui rm -rf "/My Pack/old-folder"       # permanent delete
pikpaktui rm -n "/My Pack/file.txt"          # dry run
pikpaktui rm -n -rf "/My Pack/folder"        # dry run permanent
```

::: warning
`-f` deletes permanently. There is no recovery. Use dry-run first.
:::

---

## mkdir

Create a new folder or a nested folder path.

```
pikpaktui mkdir [options] <parent_path> <folder_name>
pikpaktui mkdir [options] -p <full_path>
```

| Flag | Description |
|------|-------------|
| `-p` | Create all intermediate directories in `<full_path>` |
| `-n`, `--dry-run` | Preview without executing |

**Examples:**

```bash
pikpaktui mkdir "/My Pack" NewFolder          # create one folder
pikpaktui mkdir -p "/My Pack/a/b/c"           # create nested path
pikpaktui mkdir -n "/My Pack" NewFolder       # dry run
pikpaktui mkdir -n -p "/My Pack/a/b/c"        # dry run nested
```

::: tip
Without `-p`, the syntax is `<parent_path> <folder_name>` — two arguments.  
With `-p`, pass the full path as a single argument.
:::

---

## info

Show detailed metadata for a file or folder, including media tracks for video files.

```
pikpaktui info [options] <path>
```

| Flag | Description |
|------|-------------|
| `-J`, `--json` | JSON output (includes hash, download links, media tracks) |

**Examples:**

```bash
pikpaktui info "/My Pack/video.mp4"
pikpaktui info "/My Pack/video.mp4" --json
```

---

## link

Print the direct download URL for a file, optionally including video stream URLs.

```
pikpaktui link [options] <path>
```

| Flag | Description |
|------|-------------|
| `-m`, `--media` | Also show transcoded video stream URLs |
| `-c`, `--copy` | Copy the URL to clipboard |
| `-J`, `--json` | JSON output: `{name, url, size}` |

**Examples:**

```bash
pikpaktui link "/My Pack/file.zip"
pikpaktui link "/My Pack/file.zip" --copy       # copy to clipboard
pikpaktui link "/My Pack/video.mp4" -m          # include stream URLs
pikpaktui link "/My Pack/file.zip" --json
pikpaktui link -mc "/My Pack/video.mp4"         # media + copy
```

---

## cat

Print the text content of a file to stdout. Useful for previewing small text files or configs stored in PikPak.

```
pikpaktui cat <path>
```

**Example:**

```bash
pikpaktui cat "/My Pack/notes.txt"
```

---

## play

Stream a video file using an external player. Lists available quality options if no quality is specified.

```
pikpaktui play <path> [quality]
```

| Argument | Description |
|----------|-------------|
| `quality` | Stream quality: `720`, `1080`, `original`, or a stream index number |

**Examples:**

```bash
pikpaktui play "/My Pack/video.mp4"           # list available streams
pikpaktui play "/My Pack/video.mp4" 1080      # play 1080p
pikpaktui play "/My Pack/video.mp4" original  # play original file
pikpaktui play "/My Pack/video.mp4" 2         # play stream #2 by index
```

::: tip Player configuration
Set your player in `config.toml` (`player = "mpv"`) or via the TUI Settings panel. Any command-line video player works: `mpv`, `vlc`, `iina`, `celluloid`, etc.
:::

---

## download

Download files or entire folders recursively to local storage.

```
pikpaktui download [options] <path>
pikpaktui download [options] -t <local_dir> <path...>
```

| Flag | Description |
|------|-------------|
| `-o`, `--output <file>` | Custom output filename (single file only) |
| `-t <local_dir>` | Batch mode — download multiple items into `<local_dir>` |
| `-j`, `--jobs <n>` | Concurrent download threads (default: 1) |
| `-n`, `--dry-run` | Preview without downloading |

**Examples:**

```bash
pikpaktui download "/My Pack/file.txt"                  # to current dir
pikpaktui download "/My Pack/file.txt" /tmp/file.txt    # to specific path
pikpaktui download -o output.mp4 "/My Pack/video.mp4"   # custom name
pikpaktui download "/My Pack/folder"                    # recursive folder
pikpaktui download -j4 -t ./videos/ /a.mp4 /b.mp4      # 4 concurrent, batch
pikpaktui download -n "/My Pack/folder"                 # dry run
```

::: tip Concurrent downloads
`-j` / `--jobs` sets the number of parallel download threads. Values of 2–4 are typical; configurable in `config.toml` as `download_jobs`.
:::

---

## upload

Upload local files to PikPak. Supports deduplication (instant if file already exists server-side) and resumable uploads.

```
pikpaktui upload [options] <local_path> [remote_path]
pikpaktui upload [options] -t <remote_dir> <local...>
```

| Flag | Description |
|------|-------------|
| `[remote_path]` | Optional destination folder (positional, single file only) |
| `-t <remote_dir>` | Batch mode — upload multiple files into `<remote_dir>` |
| `-n`, `--dry-run` | Preview without uploading |

**Examples:**

```bash
pikpaktui upload ./file.txt                      # upload to root (/)
pikpaktui upload ./file.txt "/My Pack"           # upload to specific folder
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt   # batch upload
pikpaktui upload -n ./file.txt "/My Pack"        # dry run
```

::: tip Deduplication
If you upload a file that already exists on PikPak (matching hash), the upload completes instantly — no data transfer occurs.
:::

---

## share

Create, list, save, and delete share links.

```
pikpaktui share [options] <path...>      # create
pikpaktui share -l                       # list your shares
pikpaktui share -S <url>                 # save a share to your drive
pikpaktui share -D <id...>               # delete share(s)
```

**Create options:**

| Flag | Description |
|------|-------------|
| `-p`, `--password` | Auto-generate a password for the share |
| `-d`, `--days <n>` | Expiry in days; `-1` = permanent (default) |
| `-o <file>` | Write share URL to a file |
| `-J`, `--json` | JSON output: `{share_id, share_url, pass_code}` |

**Save options (with `-S`):**

| Flag | Description |
|------|-------------|
| `-p <code>` | Pass code for a password-protected share |
| `-t <path>` | Destination folder in your drive |
| `-n`, `--dry-run` | Preview without saving |

**Examples:**

```bash
pikpaktui share "/My Pack/file.txt"               # create plain share
pikpaktui share -p "/My Pack/file.txt"            # password-protected
pikpaktui share -d 7 "/My Pack/file.txt"          # expires in 7 days
pikpaktui share -p -d 7 /a.txt /b.txt             # multiple files, password, 7 days
pikpaktui share -J "/My Pack/file.txt"            # JSON output

pikpaktui share -l                                # list all your shares
pikpaktui share -l -J                             # JSON list

pikpaktui share -D abc123                         # delete one share
pikpaktui share -D abc123 def456                  # delete multiple

pikpaktui share -S "https://mypikpak.com/s/XXXX"              # save to /
pikpaktui share -S -p PO -t "/My Pack" "https://..."          # with password + destination
pikpaktui share -S -n "https://mypikpak.com/s/XXXX"           # dry run
```

---

## offline

Submit a URL or magnet link for server-side (cloud) downloading. The download runs on PikPak's servers.

```
pikpaktui offline [options] <url>
```

| Flag | Description |
|------|-------------|
| `--to`, `-t <path>` | Destination folder in PikPak |
| `--name`, `-n <name>` | Override the task/file name |
| `--dry-run` | Preview without creating the task |

**Examples:**

```bash
pikpaktui offline "magnet:?xt=urn:btih:abc123..."
pikpaktui offline --to "/Downloads" "https://example.com/file.zip"
pikpaktui offline --to "/Downloads" --name "myvideo.mp4" "https://..."
pikpaktui offline --dry-run "magnet:?xt=..."
```

---

## tasks

Manage offline download tasks.

```
pikpaktui tasks [subcommand] [options] [limit]
```

**Subcommands:**

| Subcommand | Description |
|------------|-------------|
| `list`, `ls` | List tasks (default when no subcommand given) |
| `retry <id>` | Retry a failed task |
| `delete <id...>`, `rm <id...>` | Delete task(s) |

**Options:**

| Flag | Description |
|------|-------------|
| `-J`, `--json` | JSON output for `list` |
| `-n`, `--dry-run` | Preview for `delete` |
| `<number>` | Limit number of results (default: 50) |

**Examples:**

```bash
pikpaktui tasks                             # list up to 50 tasks
pikpaktui tasks list 10                    # list 10 tasks
pikpaktui tasks list --json                # JSON output
pikpaktui tasks retry abc12345             # retry a failed task
pikpaktui tasks delete abc12345            # delete a task
pikpaktui tasks rm abc12345 def67890       # delete multiple tasks
```

---

## trash

List files currently in the trash.

```
pikpaktui trash [options] [limit]
```

| Flag / Arg | Description |
|------------|-------------|
| `-l`, `--long` | Long format — shows ID, size, date |
| `-J`, `--json` | JSON output |
| `<number>` | Max number of results (default: 100) |

**Examples:**

```bash
pikpaktui trash                  # list up to 100 trashed files
pikpaktui trash 50               # limit to 50
pikpaktui trash -l               # long format
pikpaktui trash --json           # JSON output
```

---

## untrash

Restore one or more files from trash by exact filename.

```
pikpaktui untrash [options] <name...>
```

| Flag | Description |
|------|-------------|
| `-n`, `--dry-run` | Preview without restoring |

**Examples:**

```bash
pikpaktui untrash "file.txt"
pikpaktui untrash "a.txt" "b.mp4"       # restore multiple
pikpaktui untrash -n "file.txt"         # dry run
```

::: tip
Match is by exact filename, not by path. If multiple trashed files share the same name, the first match is restored.
:::

---

## star

Star (bookmark) one or more files.

```
pikpaktui star <path...>
```

**Examples:**

```bash
pikpaktui star "/My Pack/video.mp4"
pikpaktui star "/My Pack/a.txt" "/My Pack/b.txt"
```

---

## unstar

Remove the star from one or more files.

```
pikpaktui unstar <path...>
```

**Example:**

```bash
pikpaktui unstar "/My Pack/video.mp4"
```

---

## starred

List all starred files.

```
pikpaktui starred [options] [limit]
```

| Flag / Arg | Description |
|------------|-------------|
| `-l`, `--long` | Long format |
| `-J`, `--json` | JSON output |
| `<number>` | Max results (default: 100) |

**Examples:**

```bash
pikpaktui starred
pikpaktui starred 50
pikpaktui starred -l
pikpaktui starred --json
```

---

## events

List recent file activity (uploads, downloads, deletions, etc.).

```
pikpaktui events [options] [limit]
```

| Flag / Arg | Description |
|------------|-------------|
| `-J`, `--json` | JSON output |
| `<number>` | Max results (default: 20) |

**Examples:**

```bash
pikpaktui events
pikpaktui events 50
pikpaktui events --json
```

---

## login

Log in to PikPak and save credentials to `~/.config/pikpaktui/login.yaml`.

```
pikpaktui login [options]
```

| Flag | Description |
|------|-------------|
| `-u`, `--user <email>` | PikPak account email |
| `-p`, `--password <pass>` | PikPak account password |

Environment variable fallbacks (lower priority than flags):

| Variable | Description |
|----------|-------------|
| `PIKPAK_USER` | Account email |
| `PIKPAK_PASS` | Account password |

**Examples:**

```bash
pikpaktui login                                         # interactive prompt
pikpaktui login -u user@example.com -p mypassword
PIKPAK_USER=user@example.com PIKPAK_PASS=pass pikpaktui login
```

---

## quota

Show your storage quota and bandwidth usage.

```
pikpaktui quota [options]
```

| Flag | Description |
|------|-------------|
| `-J`, `--json` | JSON output |

**Examples:**

```bash
pikpaktui quota
pikpaktui quota --json
```

---

## vip

Show VIP membership status, invite code, and transfer quota.

```
pikpaktui vip
```

---

## completions

Generate shell completion scripts. Currently only **Zsh** is supported.

```
pikpaktui completions <shell>
```

**Examples:**

```bash
pikpaktui completions zsh                            # print to stdout
pikpaktui completions zsh > ~/.zfunc/_pikpaktui      # save to file
eval "$(pikpaktui completions zsh)"                  # load in current shell
```

See [Shell Completions](/guide/shell-completions) for full setup instructions.
