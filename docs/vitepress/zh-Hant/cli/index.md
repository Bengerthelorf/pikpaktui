# CLI 概覽

pikpaktui 提供 27 條 CLI 子指令，適合腳本、自動化與進階使用者。所有指令均需有效工作階段——先執行 `pikpaktui`（TUI）登入，或使用 `pikpaktui login`。

## 指令分組

### 檔案管理

| 指令 | 說明 |
|------|------|
| [`ls`](/zh-Hant/cli/commands#ls) | 列出檔案與資料夾 |
| [`mv`](/zh-Hant/cli/commands#mv) | 移動檔案或資料夾 |
| [`cp`](/zh-Hant/cli/commands#cp) | 複製檔案或資料夾 |
| [`rename`](/zh-Hant/cli/commands#rename) | 重新命名檔案或資料夾 |
| [`rm`](/zh-Hant/cli/commands#rm) | 刪除至回收桶（`-f` 永久刪除） |
| [`mkdir`](/zh-Hant/cli/commands#mkdir) | 建立資料夾 |
| [`info`](/zh-Hant/cli/commands#info) | 檢視檔案/資料夾詳細元資料 |
| [`link`](/zh-Hant/cli/commands#link) | 取得直連網址 |
| [`cat`](/zh-Hant/cli/commands#cat) | 預覽文字檔案內容 |

### 播放

| 指令 | 說明 |
|------|------|
| [`play`](/zh-Hant/cli/commands#play) | 以外部播放器線上播放影片 |

### 傳輸

| 指令 | 說明 |
|------|------|
| [`download`](/zh-Hant/cli/commands#download) | 下載檔案或資料夾 |
| [`upload`](/zh-Hant/cli/commands#upload) | 上傳檔案至 PikPak |
| [`share`](/zh-Hant/cli/commands#share) | 建立、列出、儲存或刪除分享連結 |

### 離線下載

| 指令 | 說明 |
|------|------|
| [`offline`](/zh-Hant/cli/commands#offline) | 提交 URL 或磁力連結進行雲端下載 |
| [`tasks`](/zh-Hant/cli/commands#tasks) | 管理離線下載任務 |

### 回收桶

| 指令 | 說明 |
|------|------|
| [`trash`](/zh-Hant/cli/commands#trash) | 列出回收桶中的檔案 |
| [`untrash`](/zh-Hant/cli/commands#untrash) | 依檔案名稱從回收桶復原檔案 |

### 加星號與活動記錄

| 指令 | 說明 |
|------|------|
| [`star`](/zh-Hant/cli/commands#star) | 加星號 |
| [`unstar`](/zh-Hant/cli/commands#unstar) | 取消加星號 |
| [`starred`](/zh-Hant/cli/commands#starred) | 列出已加星號的檔案 |
| [`events`](/zh-Hant/cli/commands#events) | 最近檔案操作記錄 |

### 驗證

| 指令 | 說明 |
|------|------|
| [`login`](/zh-Hant/cli/commands#login) | 登入並儲存憑證 |

### 帳戶

| 指令 | 說明 |
|------|------|
| [`quota`](/zh-Hant/cli/commands#quota) | 儲存空間與頻寬配額 |
| [`vip`](/zh-Hant/cli/commands#vip) | VIP 狀態與帳戶資訊 |

### 工具程式

| 指令 | 說明 |
|------|------|
| [`update`](/zh-Hant/cli/commands#update) | 檢查更新並自動更新二進位檔案 |
| [`completions`](/zh-Hant/cli/commands#completions) | 產生 Shell 補全腳本 |

## 常用參數

### JSON 輸出

大多數清單類指令支援 `-J` / `--json`，輸出機器可讀格式，便於管道傳給 `jq`：

```bash
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'
pikpaktui info "/My Pack/video.mp4" --json
pikpaktui quota --json
```

### Dry run 預覽

所有修改資料的指令均支援 `-n` / `--dry-run`，解析路徑後顯示操作計畫，不做實際變更：

```bash
pikpaktui rm -n "/My Pack/file.txt"
pikpaktui mv -n "/My Pack/a.txt" /Archive
pikpaktui download -n "/My Pack/folder"
pikpaktui upload -n ./file.txt "/My Pack"
```

### 批次模式（`-t`）

`mv`、`cp`、`download`、`upload` 支援 `-t <目標>` 對多個檔案批次操作：

```bash
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt
pikpaktui download -t ./local/ /a.mp4 /b.mp4
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt
```
