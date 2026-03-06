# Project Structure

```
src/
  main.rs                Entry point — CLI dispatch or TUI launch
  config.rs              Credentials (login.yaml), TUI settings (config.toml)
  pikpak.rs              PikPak REST API client (auth, drive ops, upload, offline, VIP)
  theme.rs               File categorization, icons, color schemes
  cmd/
    mod.rs               Shared CLI helpers (client init, path resolution)
    help.rs              Colored ASCII-art help banner
    ls.rs                ls — colored grid / long format / tree
    mv.rs                mv — move files
    cp.rs                cp — copy files
    rename.rs            rename — rename files
    rm.rs                rm — trash / permanent delete
    mkdir.rs             mkdir — create folder
    download.rs          download — download to local
    upload.rs            upload — resumable dedup-aware upload
    share.rs             share — create/list/delete shares; save shared links
    quota.rs             quota — storage and bandwidth usage
    offline.rs           offline — submit URL/magnet download
    tasks.rs             tasks — manage offline tasks
    star.rs              star — star files
    unstar.rs            unstar — unstar files
    starred.rs           starred — list starred files
    events.rs            events — recent activity
    trash.rs             trash — list trashed files
    untrash.rs           untrash — restore from trash
    info.rs              info — detailed file/folder info
    link.rs              link — get direct download URL, copy to clipboard
    cat.rs               cat — text file preview
    play.rs              play — video playback via external player
    vip.rs               vip — VIP status and invite code
    completions.rs       completions — shell completion script generator
    complete_path.rs     __complete_path — internal dynamic path completion helper
  tui/
    mod.rs               App state, event loop, Miller columns, syntax highlighting
    draw.rs              All rendering (login, 3-column layout, overlays, settings)
    handler.rs           Keyboard and mouse input handling
    completion.rs        Remote cloud path tab-completion (for move/copy input)
    local_completion.rs  Local filesystem path tab-completion (for download destination)
    download.rs          Download manager (task queue, workers, pause/resume, persistence)
    download_view.rs     Download UI (collapsed popup / expanded full-screen with network graph)
    image_render.rs      Terminal image rendering
    widgets.rs           Custom widgets
```
