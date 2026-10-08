---
title: 設定
section: guide
order: 3
locale: zh-Hant
---


所有設定檔位於 `~/.config/pikpaktui/` 目錄下。

## 帳號憑證——`login.toml`

儲存 PikPak 帳號資訊，首次登入（TUI 或 `pikpaktui login`）時自動建立。

```toml
username = "you@example.com"
password = "your-password"
```

也可透過環境變數傳入（用於 `login` 指令）：

```bash
PIKPAK_USER=you@example.com PIKPAK_PASS=yourpassword pikpaktui login
```

:::callout[warning]{kind="warn"}
憑證以純文字儲存，請確保 `~/.config/pikpaktui/` 目錄權限為 `chmod 700`。
:::

## TUI 與 CLI 設定——`config.toml`

主設定檔，可手動編輯，也可在 TUI 設定面板（`,`）中修改後按 `s` 儲存。
這些純量設定是 TOML 頂層欄位，請勿放入 `[tui]` 表。

```toml
# 介面
nerd_font = false           # TUI 中使用 Nerd Font 圖示（需要 Nerd Font 終端機字型）
border_style = "thick"      # "rounded" | "thick" | "thick-rounded" | "double"
color_scheme = "vibrant"    # "vibrant" | "classic" | "custom"
show_help_bar = true        # 底部快捷鍵提示列
quota_bar_style = "bar"     # "bar"（視覺化進度條）| "percent"（百分比數字）

# 檔案瀏覽
columns = "auto"           # "auto" 或正整數（4、5、6……）
column_min_width = 28       # 最小欄寬，單位為終端機字元格，至少 16
inline_thumbnails = true   # 檔名前的小縮圖；v 開關
inline_thumbnail_size = "tiny"  # "tiny" | "small" | "medium" | "large"；[ / ] 調整
thumbnail_placeholders = true  # 等待或不可用時保留灰色圖片佔位卡

# 預覽
show_preview = true         # 寬度允許時顯示預覽；false = 僅檔案瀏覽面板
lazy_preview = false        # 游標停止後自動載入文字預覽和檔案詳情
preview_max_size = 65536    # 文字預覽最大載入位元組數（預設 64 KB）
thumbnail_mode = "auto"     # "auto" | "off" | "force-color" | "force-grayscale"
thumbnail_size = "medium"   # 伺服器圖片清晰度，與列表顯示尺寸分開設定; "small" | "medium" | "large"

# 排序（在 TUI 中用 S / R 修改時自動儲存）
sort_field = "name"         # "name" | "size" | "created" | "type" | "extension" | "none"
sort_reverse = false

# 互動
move_mode = "picker"        # "picker"（雙面板圖形選擇器）| "input"（文字輸入 + Tab 補全）
cli_nerd_font = false       # CLI 輸出中使用 Nerd Font 圖示

# 播放
player = "mpv"              # 外部影片播放器指令；首次播放影片時在 TUI 設定
playback_quality = "original" # TUI 預設播放畫質；original | 2160p | 1440p | 1080p | 720p | 480p | 360p

# 下載
download_jobs = 1           # TUI 下載工作執行緒數（設定介面範圍 1–16）
update_check = "notify"    # "notify" | "quiet" | "off"
```

預設播放偏好為 `original`（原畫），代表來源檔案的畫質，不固定為 1080p。可在 **Playback Settings → Default Playback Quality** 修改並按 `s` 儲存，或設定頂層 `playback_quality`；`4k` 是 `2160p` 的別名。指定解析度時，優先選擇允許播放且不高於目標的最高轉碼版本，其次選擇高於目標的最低版本；沒有已知解析度的可用轉碼時退回原畫／首個允許播放的連結。播放確認框顯示實際選項，偏好不可用時提示退回。下拉選單的單次選擇不修改預設設定。CLI `play` 仍在省略畫質參數時列出版本，提供名稱／序號時依該參數播放。

### update_check

控制更新檢查行為。

- `"notify"`（預設）— 啟動時檢查更新，在 TUI 狀態列和 CLI stderr 中持續顯示
- `"quiet"` — 靜默檢查，僅在 TUI 日誌中顯示
- `"off"` — 完全停用更新檢查

```toml
update_check = "notify"
```

### 縮圖設定

**Browser Settings** 提供欄目數、最小欄寬、行內縮圖開關、顯示尺寸、佔位圖開關與伺服器縮圖尺寸。這些欄位都放在 `config.toml` 頂層。

`inline_thumbnail_size` 控制列表圖片佔多少終端字元格；`thumbnail_size` 控制伺服器圖片清晰度（`small`、`medium`、`large`）。儲存 **Thumbnail Source Size** 後，共用背景工作立即使用新尺寸，並重新整理瀏覽器。

`thumbnail_placeholders = true` 預設開啟，舊設定沒有這個欄位也會開啟。未載入時顯示低對比度灰色卡片，載入中顯示小轉動指示，不可用時顯示叉號；沒有縮圖連結的圖片、影片也保留不可用卡片。列高與檔名位置保持穩定。設為 `false` 可恢復普通圖示回退。`inline_thumbnails` 控制列表圖片，`show_preview` 控制預覽面板，`thumbnail_mode` 控制大預覽的圖片繪製。

播放連結選擇、工作完成確認、憑據更新及過期回應修復自動生效，無需額外相容開關。

### 圖片協定設定

依終端機模擬器設定圖片渲染協定，鍵名為 `$TERM_PROGRAM` 環境變數的值。首次使用該終端機時自動新增項目。

```toml
[image_protocols]
ghostty = "kitty"
"iTerm.app" = "iterm2"
WezTerm = "auto"
```

可選值：`"auto"`（自動偵測）、`"kitty"`、`"iterm2"`、`"sixel"`。

### 自訂色彩

當 `color_scheme = "custom"` 時生效，每個值為 `[R, G, B]` 陣列（0–255）。

```toml
[custom_colors]
folder   = [92, 176, 255]   # 淡藍
archive  = [255, 102, 102]  # 淡紅
image    = [255, 102, 255]  # 淡品紅
video    = [102, 255, 255]  # 淡青
audio    = [0, 255, 255]    # 青色
document = [102, 255, 102]  # 淡綠
code     = [255, 255, 102]  # 淡黃
default  = [255, 255, 255]  # 白色
```

在 TUI 中編輯自訂色彩：開啟設定（`,`）→ 選取 **Color Scheme** → 按 `Enter` 進入色彩編輯器 → 用 `r` / `g` / `b` 分別編輯 RGB 分量。

## 自動管理的檔案

以下檔案由 pikpaktui 自動維護，無需手動編輯。

| 檔案 | 說明 |
|------|------|
| `session.json` | 存取權杖與更新權杖（自動更新） |
| `downloads.json` | 未完成的下載狀態（重新啟動後可恢復） |

## 環境變數

環境變數優先順序高於設定檔，適用於 CI 或臨時覆寫。

| 變數 | 說明 |
|------|------|
| `PIKPAK_USER` | 帳號電子郵件（`pikpaktui login` 使用） |
| `PIKPAK_PASS` | 帳號密碼（`pikpaktui login` 使用） |
| `PIKPAK_DRIVE_BASE_URL` | 覆寫 PikPak Drive API 位址 |
| `PIKPAK_AUTH_BASE_URL` | 覆寫 PikPak 驗證 API 位址 |
| `PIKPAK_CLIENT_ID` | 覆寫 OAuth Client ID |
| `PIKPAK_CLIENT_SECRET` | 覆寫 OAuth Client Secret |
| `PIKPAK_CAPTCHA_TOKEN` | 登入遭遇驗證碼時提供 token |

:::callout[並行下載]{kind="info"}
`download_jobs` 僅控制 TUI 下載佇列；CLI 遞迴下載資料夾請使用 `download -j <n>`。通常建議設為 2–4，TUI 設定介面可選 1–16。
:::
