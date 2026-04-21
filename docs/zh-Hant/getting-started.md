---
title: 快速開始
section: guide
order: 1
locale: zh-Hant
---


pikpaktui 是一個 [PikPak](https://mypikpak.com) 雲端儲存的終端機客戶端，提供互動式 TUI 與包含 27 條子指令的完整 CLI。由純 Rust 撰寫，無 OpenSSL，無 C 相依。

## 系統需求

- PikPak 帳號（[立即註冊](https://mypikpak.com)）
- macOS（Intel 或 Apple Silicon）、Linux（x86_64 / ARM64）、Windows（x86_64 / ARM64）或 FreeBSD

## 安裝

:::code-group

```bash [安裝腳本]
curl -fsSL https://app.snaix.homes/pikpaktui/install.sh | bash
```

```bash [Homebrew]
brew install Bengerthelorf/tap/pikpaktui
```

```bash [Cargo]
cargo install pikpaktui
```

```bash [從原始碼建置]
git clone https://github.com/Bengerthelorf/pikpaktui.git
cd pikpaktui
cargo build --release
# 二進位檔位於 ./target/release/pikpaktui
```

:::

預先編譯的二進位檔（含 Linux musl 靜態版）可至 [Releases 頁面](https://github.com/Bengerthelorf/pikpaktui/releases/latest)下載。

## 首次啟動——TUI

不帶參數執行即可開啟互動式 TUI：

```bash
pikpaktui
```

首次執行會顯示登入表單，輸入 PikPak 電子郵件與密碼。登入成功後，憑證儲存至 `~/.config/pikpaktui/login.yaml`，工作階段儲存至 `~/.config/pikpaktui/session.json`，之後無需再次登入（會自動更新）。

![TUI 主畫面](/images/main.jpeg)

:::callout[快捷鍵速查]{kind="info"}
- `,` — 開啟設定
- `h` — 顯示完整快捷鍵說明
- `q` — 離開
:::

## TUI 導覽

TUI 為三欄 Miller 式佈局：

| 面板 | 內容 |
|------|------|
| **左側** | 上層目錄 |
| **中間** | 目前目錄（主操作區） |
| **右側** | 預覽（縮圖 / 文字 / 子目錄清單） |

### 基本導覽

| 按鍵 | 操作 |
|------|------|
| `j` / `k` 或 `↑` / `↓` | 上下移動游標 |
| `Enter` | 進入資料夾 / 播放影片 |
| `Backspace` | 返回上層目錄 |
| `g` / `Home` | 跳至頂端 |
| `G` / `End` | 跳至底部 |
| `Ctrl+U` / `Ctrl+D` | 半頁捲動 |
| `r` | 重新整理目前目錄 |
| `:` | 輸入路徑跳轉 |

### 檔案操作

| 按鍵 | 操作 |
|------|------|
| `m` | 移動（依 `move_mode` 設定使用資料夾選擇器或文字輸入） |
| `c` | 複製 |
| `n` | 重新命名（內嵌輸入框） |
| `d` | 刪除（確認提示：`y` 移至回收桶，`p` 永久刪除） |
| `f` | 新增資料夾 |
| `s` | 加星號 / 取消加星號 |
| `y` | 複製直連網址至剪貼簿（僅限檔案） |

### 檢視與功能

| 按鍵 | 操作 |
|------|------|
| `a` | 將目前檔案加入/移出購物車 |
| `A` | 開啟購物車（批次下載/移動/複製） |
| `u` | 上傳本機檔案至目前資料夾 |
| `w` | 線上播放影片——開啟畫質選擇器 |
| `o` | 離線下載——輸入 URL 或磁力連結 |
| `O` | 離線任務檢視 |
| `D` | 下載管理檢視 |
| `t` | 回收桶檢視 |
| `M` | 我的分享檢視 |
| `Space` | 檔案/資料夾詳細資訊彈窗 |
| `l` | 切換記錄面板 |

詳見 [TUI 指南](/zh-Hant/guide/tui)。

## CLI 快速上手

所有 CLI 子指令均需有效工作階段（先執行 `pikpaktui` 登入，或使用 `pikpaktui login`）。

```bash
# 列出檔案
pikpaktui ls /
pikpaktui ls -l "/My Pack"        # 長格式（含大小與日期）
pikpaktui ls --tree --depth=2 /   # 遞迴樹狀檢視

# 檔案操作
pikpaktui mv "/My Pack/file.txt" /Archive
pikpaktui cp "/My Pack/video.mp4" /Backup
pikpaktui rename "/My Pack/old.txt" new.txt
pikpaktui rm "/My Pack/file.txt"           # 移至回收桶
pikpaktui rm -rf "/My Pack/old-folder"    # 永久刪除

# 傳輸
pikpaktui download "/My Pack/video.mp4"
pikpaktui download -j4 -t ./videos/ /a.mp4 /b.mp4  # 4 並行
pikpaktui upload ./notes.txt "/My Pack"

# 離線下載
pikpaktui offline "magnet:?xt=urn:btih:..."
pikpaktui offline --to "/Downloads" "https://example.com/file.zip"
```

:::callout[Dry run 預覽]{kind="info"}
所有修改資料的指令均支援 `-n` / `--dry-run`——解析路徑後顯示操作計畫，不做任何實際變更。
:::

## 透過 CLI 登入

適合腳本或 CI 環境的非互動式登入：

```bash
pikpaktui login -u you@example.com -p yourpassword

# 也可透過環境變數：
PIKPAK_USER=you@example.com PIKPAK_PASS=yourpassword pikpaktui login
```

## 環境變數

| 變數 | 說明 |
|------|------|
| `PIKPAK_USER` | 帳號電子郵件（`login` 指令使用） |
| `PIKPAK_PASS` | 帳號密碼（`login` 指令使用） |
| `PIKPAK_DRIVE_BASE_URL` | 覆寫 PikPak Drive API 位址 |
| `PIKPAK_AUTH_BASE_URL` | 覆寫 PikPak 驗證 API 位址 |
| `PIKPAK_CLIENT_ID` | 覆寫 OAuth Client ID |
| `PIKPAK_CLIENT_SECRET` | 覆寫 OAuth Client Secret |
| `PIKPAK_CAPTCHA_TOKEN` | 登入遭遇驗證碼時提供 token |

## 下一步

- [TUI 指南](/zh-Hant/guide/tui) — 所有檢視的完整快捷鍵參考
- [設定](/zh-Hant/guide/configuration) — 自訂配色、字型、播放器等
- [CLI 指令參考](/zh-Hant/cli/commands) — 全部 27 條指令詳解
- [Shell 補全](/zh-Hant/guide/shell-completions) — 在 zsh 中 Tab 補全雲端路徑
