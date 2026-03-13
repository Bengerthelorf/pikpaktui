---
layout: home
hero:
  name: pikpaktui
  text: 在終端機管理 PikPak
  tagline: 快速、鍵盤驅動的 PikPak 雲端儲存 TUI 與 CLI 客戶端。瀏覽、管理、下載、線上播放——無需離開終端機。
  actions:
    - theme: brand
      text: 快速開始
      link: /zh-Hant/guide/getting-started
    - theme: alt
      text: CLI 參考
      link: /zh-Hant/cli/
    - theme: alt
      text: GitHub
      link: https://github.com/Bengerthelorf/pikpaktui

features:
  - icon: 🖥️
    title: 互動式 TUI
    details: 三欄 Miller 式佈局（類似 Yazi），可瀏覽資料夾、預覽縮圖與文字，移動/複製/重新命名，線上播放影片，管理離線下載——全鍵盤操作。
  - icon: ⌨️
    title: 完整 CLI，26 條指令
    details: ls、mv、cp、rm、download、upload、share、offline、tasks 等。支援 JSON 輸出便於腳本化，--dry-run 預覽操作，-j 並行下載。
  - icon: 🦀
    title: 純 Rust，無外部相依
    details: 基於 ratatui + crossterm + reqwest (rustls-tls)，無 OpenSSL，無 C 相依。提供 Linux x86_64 靜態 musl 二進位檔。
  - icon: 🌐
    title: 離線（雲端）下載
    details: 提交磁力連結與 HTTP/HTTPS 網址，由 PikPak 伺服器完成下載。可在 TUI 或 CLI 中監控進度、重試失敗任務。
  - icon: 📤
    title: 分享與傳輸
    details: 建立帶可選密碼與有效期的分享連結。支援將他人分享的內容直接儲存至網盤，dry-run 模式可預覽操作。
  - icon: 🔧
    title: 高度可設定
    details: Nerd Font 圖示、自訂 RGB 配色方案、依終端機設定圖片協定（Kitty/iTerm2/Sixel）、並行下載數、外部播放器等。
---
