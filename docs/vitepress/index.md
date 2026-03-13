---
layout: home
hero:
  name: pikpaktui
  text: PikPak in Your Terminal
  tagline: A fast, keyboard-driven TUI and CLI client for PikPak cloud storage. Browse, manage, download, and stream — without leaving the terminal.
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: CLI Reference
      link: /cli/
    - theme: alt
      text: View on GitHub
      link: https://github.com/Bengerthelorf/pikpaktui

features:
  - icon: 🖥️
    title: Interactive TUI
    details: Three-column Miller layout (like Yazi) — navigate folders, preview thumbnails and text files, move/copy/rename, stream videos, and manage offline downloads, all from the keyboard.
  - icon: ⌨️
    title: Full CLI — 26 Commands
    details: ls, mv, cp, rm, download, upload, share, offline, tasks, and more. JSON output for scripting, dry-run to preview changes, concurrent downloads with -j.
  - icon: 🦀
    title: Pure Rust, No Dependencies
    details: Built on ratatui + crossterm + reqwest (rustls-tls). No OpenSSL, no C dependencies. Static musl binary available for Linux x86_64.
  - icon: 🌐
    title: Offline (Cloud) Downloads
    details: Submit magnet links and HTTP/HTTPS URLs for server-side downloading. Monitor progress, retry failures, all within the TUI or via CLI.
  - icon: 📤
    title: Share & Transfer
    details: Create share links with optional password protection and expiry. Save shares from others directly to your drive, with dry-run preview.
  - icon: 🔧
    title: Fully Configurable
    details: Nerd Font icons, custom RGB color schemes, per-terminal image protocols (Kitty/iTerm2/Sixel), concurrent download jobs, external video player, and more.
---
