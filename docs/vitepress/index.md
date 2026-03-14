---
layout: home
hero:
  name: pikpaktui
  text: PikPak in Your Terminal
  tagline: A fast, keyboard-driven TUI and CLI client for PikPak cloud storage. Browse, manage, download, and stream — without leaving the terminal.
  image:
    src: /images/icon.svg
    alt: pikpaktui
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
    title: Full CLI — 28 Commands
    details: ls, mv, cp, rm, download, upload, share, offline, tasks, update, and more. JSON output for scripting, dry-run to preview changes, concurrent downloads with -j.
  - icon: 🦀
    title: Pure Rust, No Dependencies
    details: Built on ratatui + crossterm + reqwest (rustls-tls). No OpenSSL, no C dependencies. Pre-built binaries for macOS, Linux, Windows, and FreeBSD.
  - icon: 🌐
    title: Offline (Cloud) Downloads
    details: Submit magnet links and HTTP/HTTPS URLs for server-side downloading. Monitor progress, retry failures, all within the TUI or via CLI.
  - icon: 📤
    title: Share & Transfer
    details: Create share links with optional password protection and expiry. Save shares from others directly to your drive, with dry-run preview.
  - icon: 🔧
    title: Fully Configurable
    details: Nerd Font icons, custom RGB color schemes, per-terminal image protocols (Kitty/iTerm2/Sixel), concurrent download jobs, external video player, and more.
  - icon: 🤖
    title: AI Agent Friendly
    details: Non-interactive login, JSON output, dry-run support, and clear exit codes — designed to work with OpenClaw and other AI agents out of the box.
  - icon: 💚
    title: Open Source
    details: Apache-2.0 licensed. Contributions welcome — issues, PRs, and feature requests on GitHub. Self-updating binary keeps you on the latest version.
---
