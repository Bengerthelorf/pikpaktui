---
title: 設定
section: guide
order: 3
locale: zh-Hant
---


所有設定檔位於 `~/.config/pikpaktui/` 目錄下。

## 帳號憑證——`login.yaml`

儲存 PikPak 帳號資訊，首次登入（TUI 或 `pikpaktui login`）時自動建立。

```yaml
username: "you@example.com"
password: "your-password"
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

```toml
[tui]
# 介面
nerd_font = false           # TUI 中使用 Nerd Font 圖示（需要 Nerd Font 終端機字型）
border_style = "thick"      # "rounded" | "thick" | "thick-rounded" | "double"
color_scheme = "vibrant"    # "vibrant" | "classic" | "custom"
show_help_bar = true        # 底部快捷鍵提示列
quota_bar_style = "bar"     # "bar"（視覺化進度條）| "percent"（百分比數字）

# 預覽
show_preview = true         # 三欄佈局；false = 兩欄佈局
lazy_preview = false        # 僅在游標停止移動後載入預覽
preview_max_size = 65536    # 文字預覽最大載入位元組數（預設 64 KB）
thumbnail_mode = "auto"     # "auto" | "off" | "force-color" | "force-grayscale"
thumbnail_size = "medium"   # "small" | "medium" | "large"

# 排序（在 TUI 中用 S / R 修改時自動儲存）
sort_field = "name"         # "name" | "size" | "created" | "type" | "extension" | "none"
sort_reverse = false

# 互動
move_mode = "picker"        # "picker"（雙面板圖形選擇器）| "input"（文字輸入 + Tab 補全）
cli_nerd_font = false       # CLI 輸出中使用 Nerd Font 圖示

# 播放
player = "mpv"              # 外部影片播放器指令；首次播放影片時在 TUI 設定

# 下載
download_jobs = 1           # 並行下載執行緒數（1–16）
update_check = "notify"    # "notify" | "quiet" | "off"
```

### update_check

控制更新檢查行為。

- `"notify"`（預設）— 啟動時檢查更新，在 TUI 狀態列和 CLI stderr 中持續顯示
- `"quiet"` — 靜默檢查，僅在 TUI 日誌中顯示
- `"off"` — 完全停用更新檢查

```toml
update_check = "notify"
```

### 圖片協定設定

依終端機模擬器設定圖片渲染協定，鍵名為 `$TERM_PROGRAM` 環境變數的值。首次使用該終端機時自動新增項目。

```toml
[tui.image_protocols]
ghostty = "kitty"
"iTerm.app" = "iterm2"
WezTerm = "auto"
```

可選值：`"auto"`（自動偵測）、`"kitty"`、`"iterm2"`、`"sixel"`。

### 自訂色彩

當 `color_scheme = "custom"` 時生效，每個值為 `[R, G, B]` 陣列（0–255）。

```toml
[tui.custom_colors]
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
將 `download_jobs` 設為 2–4 通常可以顯著提升大量下載速度，最大值為 16。
:::
