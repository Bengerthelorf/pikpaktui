---
layout: home
hero:
  name: pikpaktui
  text: 在终端管理 PikPak
  tagline: 快速、键盘驱动的 PikPak 云存储 TUI 和 CLI 客户端。浏览、管理、下载、在线播放——无需离开终端。
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/getting-started
    - theme: alt
      text: CLI 参考
      link: /zh/cli/
    - theme: alt
      text: GitHub
      link: https://github.com/Bengerthelorf/pikpaktui

features:
  - icon: 🖥️
    title: 交互式 TUI
    details: 三列 Miller 式布局（类似 Yazi），可浏览文件夹、预览缩略图和文本，移动/复制/重命名，在线播放视频，管理离线下载——全键盘操作。
  - icon: ⌨️
    title: 完整 CLI，26 条命令
    details: ls、mv、cp、rm、download、upload、share、offline、tasks 等。支持 JSON 输出便于脚本化，--dry-run 预览操作，-j 并发下载。
  - icon: 🦀
    title: 纯 Rust，无外部依赖
    details: 基于 ratatui + crossterm + reqwest (rustls-tls)，无 OpenSSL，无 C 依赖。提供 Linux x86_64 静态 musl 二进制。
  - icon: 🌐
    title: 离线（云端）下载
    details: 提交磁力链接和 HTTP/HTTPS 地址，由 PikPak 服务器完成下载。可在 TUI 或 CLI 中监控进度、重试失败任务。
  - icon: 📤
    title: 分享与传输
    details: 创建带可选密码和有效期的分享链接。支持将他人分享的内容直接保存到你的网盘，dry-run 模式可预览操作。
  - icon: 🔧
    title: 高度可配置
    details: Nerd Font 图标、自定义 RGB 配色方案、按终端配置图片协议（Kitty/iTerm2/Sixel）、并发下载数、外部播放器等。
---
