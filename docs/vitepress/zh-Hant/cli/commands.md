# 指令參考

所有指令均需有效工作階段，請先執行 `pikpaktui`（TUI）登入，或使用 [`login`](#login)。

---

## ls

列出 PikPak 網盤中的檔案與資料夾。

```
pikpaktui ls [選項] [路徑]
```

| 參數 | 說明 |
|------|------|
| `-l`, `--long` | 長格式——顯示 ID、大小、日期與檔案名稱 |
| `-J`, `--json` | 輸出 JSON 陣列 |
| `-s`, `--sort <欄位>` | 按欄位排序：`name`、`size`、`created`、`type`、`extension`、`none` |
| `-r`, `--reverse` | 反向排序 |
| `--tree` | 遞迴樹狀檢視 |
| `--depth=N` | 限制樹深度為 N 層 |

**範例：**

```bash
pikpaktui ls                                # 列出根目錄
pikpaktui ls "/My Pack"                    # 列出指定資料夾
pikpaktui ls -l /Movies                    # 長格式
pikpaktui ls --sort=size -r /              # 按大小倒序
pikpaktui ls --tree --depth=2 "/My Pack"   # 樹狀，最多 2 層
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'
```

---

## mv

將檔案或資料夾移動至目標資料夾。

```
pikpaktui mv [選項] <來源路徑> <目標路徑>
pikpaktui mv [選項] -t <目標> <來源路徑...>
```

| 參數 | 說明 |
|------|------|
| `-t <目標>` | 批次模式——將多個來源移入目標資料夾 |
| `-n`, `--dry-run` | 預覽，不執行 |

**範例：**

```bash
pikpaktui mv "/My Pack/file.txt" /Archive
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt
pikpaktui mv -n "/My Pack/a.txt" /Archive
```

---

## cp

將檔案或資料夾複製至目標資料夾。

```
pikpaktui cp [選項] <來源路徑> <目標路徑>
pikpaktui cp [選項] -t <目標> <來源路徑...>
```

| 參數 | 說明 |
|------|------|
| `-t <目標>` | 批次模式 |
| `-n`, `--dry-run` | 預覽，不執行 |

**範例：**

```bash
pikpaktui cp "/My Pack/file.txt" /Backup
pikpaktui cp -t /Backup /a.txt /b.txt
```

---

## rename

原地重新命名檔案或資料夾（保持在目前目錄）。

```
pikpaktui rename [選項] <路徑> <新檔案名稱>
```

| 參數 | 說明 |
|------|------|
| `-n`, `--dry-run` | 預覽，不執行 |

**範例：**

```bash
pikpaktui rename "/My Pack/old.txt" new.txt
pikpaktui rename -n "/My Pack/old.txt" new.txt
```

---

## rm

刪除檔案或資料夾。預設移至回收桶，加 `-f` 則永久刪除。

```
pikpaktui rm [選項] <路徑...>
```

| 參數 | 說明 |
|------|------|
| `-r`, `--recursive` | 刪除資料夾時必須加此參數 |
| `-f`, `--force` | 永久刪除（略過回收桶） |
| `-rf`, `-fr` | 遞迴永久刪除資料夾 |
| `-n`, `--dry-run` | 預覽，不執行 |

**範例：**

```bash
pikpaktui rm "/My Pack/file.txt"             # 移至回收桶
pikpaktui rm /a.txt /b.txt /c.txt            # 批次刪除
pikpaktui rm -r "/My Pack/folder"            # 資料夾移至回收桶
pikpaktui rm -rf "/My Pack/old-folder"       # 永久刪除資料夾
pikpaktui rm -n -rf "/My Pack/folder"        # 預覽永久刪除
```

::: warning
`-f` 會永久刪除，無法復原。建議先以 dry-run 確認。
:::

---

## mkdir

建立資料夾或巢狀路徑。

```
pikpaktui mkdir [選項] <父路徑> <資料夾名稱>
pikpaktui mkdir [選項] -p <完整路徑>
```

| 參數 | 說明 |
|------|------|
| `-p` | 遞迴建立 `<完整路徑>` 中所有不存在的中間目錄 |
| `-n`, `--dry-run` | 預覽，不執行 |

**範例：**

```bash
pikpaktui mkdir "/My Pack" NewFolder           # 建立單一資料夾
pikpaktui mkdir -p "/My Pack/a/b/c"            # 遞迴建立巢狀路徑
pikpaktui mkdir -n "/My Pack" NewFolder        # 預覽
```

::: tip
不帶 `-p` 時語法為 `<父路徑> <資料夾名稱>`（兩個參數）；帶 `-p` 時傳完整路徑一個參數。
:::

---

## info

顯示檔案或資料夾的詳細元資料，影片檔案還包含媒體軌道資訊。

```
pikpaktui info [選項] <路徑>
```

| 參數 | 說明 |
|------|------|
| `-J`, `--json` | JSON 輸出（含 hash、下載連結、媒體軌道） |

**範例：**

```bash
pikpaktui info "/My Pack/video.mp4"
pikpaktui info "/My Pack/video.mp4" --json
```

---

## link

顯示檔案的直連下載網址，可選附帶影片串流網址。

```
pikpaktui link [選項] <路徑>
```

| 參數 | 說明 |
|------|------|
| `-m`, `--media` | 同時顯示轉碼影片串流網址 |
| `-c`, `--copy` | 複製網址至剪貼簿 |
| `-J`, `--json` | JSON 輸出：`{name, url, size}` |

**範例：**

```bash
pikpaktui link "/My Pack/file.zip"
pikpaktui link "/My Pack/file.zip" --copy
pikpaktui link "/My Pack/video.mp4" -m
pikpaktui link -mc "/My Pack/video.mp4"
```

---

## cat

將文字檔案內容輸出至標準輸出。

```
pikpaktui cat <路徑>
```

**範例：**

```bash
pikpaktui cat "/My Pack/notes.txt"
```

---

## play

以外部播放器播放影片串流。不指定畫質則列出可用選項。

```
pikpaktui play <路徑> [畫質]
```

| 參數 | 說明 |
|------|------|
| `畫質` | 串流畫質：`720`、`1080`、`original`，或按編號選擇 |

**範例：**

```bash
pikpaktui play "/My Pack/video.mp4"            # 列出可用串流
pikpaktui play "/My Pack/video.mp4" 1080       # 播放 1080p
pikpaktui play "/My Pack/video.mp4" original   # 播放原始檔案
pikpaktui play "/My Pack/video.mp4" 2          # 按編號播放第 2 條串流
```

---

## download

下載檔案或遞迴下載整個資料夾至本機。

```
pikpaktui download [選項] <路徑>
pikpaktui download [選項] -t <本機目錄> <路徑...>
```

| 參數 | 說明 |
|------|------|
| `-o`, `--output <檔案>` | 自訂輸出檔案名稱（僅單一檔案） |
| `-t <本機目錄>` | 批次模式——將多個檔案下載至指定目錄 |
| `-j`, `--jobs <n>` | 並行下載執行緒數（預設 1） |
| `-n`, `--dry-run` | 預覽，不下載 |

**範例：**

```bash
pikpaktui download "/My Pack/file.txt"
pikpaktui download -o output.mp4 "/My Pack/video.mp4"
pikpaktui download "/My Pack/folder"                      # 遞迴下載
pikpaktui download -j4 -t ./videos/ /a.mp4 /b.mp4        # 4 並行批次
pikpaktui download -n "/My Pack/folder"
```

---

## upload

上傳本機檔案至 PikPak，支援秒傳（相同 hash 已存在）與斷點續傳。

```
pikpaktui upload [選項] <本機路徑> [遠端路徑]
pikpaktui upload [選項] -t <遠端目錄> <本機檔案...>
```

| 參數 | 說明 |
|------|------|
| `[遠端路徑]` | 可選目標資料夾（單一檔案時的位置參數） |
| `-t <遠端目錄>` | 批次模式——上傳多個檔案至指定目錄 |
| `-n`, `--dry-run` | 預覽，不上傳 |

**範例：**

```bash
pikpaktui upload ./file.txt                        # 上傳至根目錄
pikpaktui upload ./file.txt "/My Pack"             # 上傳至指定資料夾
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt     # 批次上傳
pikpaktui upload -n ./file.txt "/My Pack"          # 預覽
```

---

## share

建立、列出、儲存和刪除分享連結。

```
pikpaktui share [選項] <路徑...>      # 建立
pikpaktui share -l                    # 列出我的分享
pikpaktui share -S <URL>              # 儲存他人分享至網盤
pikpaktui share -D <ID...>            # 刪除分享
```

**建立選項：**

| 參數 | 說明 |
|------|------|
| `-p`, `--password` | 自動產生存取密碼 |
| `-d`, `--days <n>` | 有效期（天）；`-1` 表示永久（預設） |
| `-o <檔案>` | 將分享網址寫入檔案 |
| `-J`, `--json` | JSON 輸出：`{share_id, share_url, pass_code}` |

**儲存選項（與 `-S` 搭配）：**

| 參數 | 說明 |
|------|------|
| `-p <密碼>` | 加密分享的存取碼 |
| `-t <路徑>` | 儲存目標資料夾 |
| `-n`, `--dry-run` | 預覽，不儲存 |

**範例：**

```bash
pikpaktui share "/My Pack/file.txt"              # 建立一般分享
pikpaktui share -p "/My Pack/file.txt"           # 加密分享
pikpaktui share -d 7 -p /a.txt /b.txt            # 多檔案+加密+7天

pikpaktui share -l                               # 列出我的分享
pikpaktui share -D abc123                        # 刪除指定分享

pikpaktui share -S "https://mypikpak.com/s/XXXX"
pikpaktui share -S -p PO -t "/My Pack" "https://..."
pikpaktui share -S -n "https://mypikpak.com/s/XXXX"   # 預覽
```

---

## offline

提交 URL 或磁力連結進行伺服器端（雲端）下載。

```
pikpaktui offline [選項] <URL>
```

| 參數 | 說明 |
|------|------|
| `--to`, `-t <路徑>` | PikPak 中的目標資料夾 |
| `--name`, `-n <名稱>` | 覆寫任務/檔案名稱 |
| `--dry-run` | 預覽，不建立任務 |

**範例：**

```bash
pikpaktui offline "magnet:?xt=urn:btih:abc123..."
pikpaktui offline --to "/Downloads" "https://example.com/file.zip"
pikpaktui offline --to "/Downloads" --name "myvideo.mp4" "https://..."
pikpaktui offline --dry-run "magnet:?xt=..."
```

---

## tasks

管理離線下載任務。

```
pikpaktui tasks [子指令] [選項] [數量]
```

**子指令：**

| 子指令 | 說明 |
|--------|------|
| `list`、`ls` | 列出任務（不指定時的預設行為） |
| `retry <ID>` | 重試失敗的任務 |
| `delete <ID...>`、`rm <ID...>` | 刪除任務 |

**選項：**

| 參數 | 說明 |
|------|------|
| `-J`, `--json` | JSON 輸出（用於 `list`） |
| `-n`, `--dry-run` | 預覽（用於 `delete`） |
| `<數字>` | 限制結果數量（預設 50） |

**範例：**

```bash
pikpaktui tasks
pikpaktui tasks list 10
pikpaktui tasks list --json
pikpaktui tasks retry abc12345
pikpaktui tasks delete abc12345
pikpaktui tasks rm abc12345 def67890
```

---

## trash

列出回收桶中的檔案。

```
pikpaktui trash [選項] [數量]
```

| 參數 | 說明 |
|------|------|
| `-l`, `--long` | 長格式 |
| `-J`, `--json` | JSON 輸出 |
| `<數字>` | 最大結果數（預設 100） |

**範例：**

```bash
pikpaktui trash
pikpaktui trash 50
pikpaktui trash -l
pikpaktui trash --json
```

---

## untrash

依精確檔案名稱從回收桶復原檔案。

```
pikpaktui untrash [選項] <檔案名稱...>
```

| 參數 | 說明 |
|------|------|
| `-n`, `--dry-run` | 預覽，不復原 |

**範例：**

```bash
pikpaktui untrash "file.txt"
pikpaktui untrash "a.txt" "b.mp4"
pikpaktui untrash -n "file.txt"
```

---

## star / unstar / starred

```bash
pikpaktui star "/My Pack/video.mp4"            # 加星號
pikpaktui star "/My Pack/a.txt" "/My Pack/b.txt"
pikpaktui unstar "/My Pack/video.mp4"          # 取消加星號

pikpaktui starred                              # 列出全部（最多 100 筆）
pikpaktui starred 50                           # 最多 50 筆
pikpaktui starred -l                           # 長格式
pikpaktui starred --json                       # JSON 輸出
```

---

## events

列出最近的檔案操作記錄。

```
pikpaktui events [選項] [數量]
```

| 參數 | 說明 |
|------|------|
| `-J`, `--json` | JSON 輸出 |
| `<數字>` | 最大結果數（預設 20） |

**範例：**

```bash
pikpaktui events
pikpaktui events 50
pikpaktui events --json
```

---

## login

登入 PikPak 並將憑證儲存至 `~/.config/pikpaktui/login.yaml`。

```
pikpaktui login [選項]
```

| 參數 | 說明 |
|------|------|
| `-u`, `--user <電子郵件>` | PikPak 帳號電子郵件 |
| `-p`, `--password <密碼>` | PikPak 帳號密碼 |

環境變數（優先順序低於指令行參數）：

| 變數 | 說明 |
|------|------|
| `PIKPAK_USER` | 帳號電子郵件 |
| `PIKPAK_PASS` | 帳號密碼 |

**範例：**

```bash
pikpaktui login
pikpaktui login -u user@example.com -p mypassword
PIKPAK_USER=user@example.com PIKPAK_PASS=pass pikpaktui login
```

---

## quota

顯示儲存空間與頻寬配額。

```
pikpaktui quota [選項]
```

| 參數 | 說明 |
|------|------|
| `-J`, `--json` | JSON 輸出 |

**範例：**

```bash
pikpaktui quota
pikpaktui quota --json
```

---

## vip

顯示 VIP 會員狀態、邀請碼與傳輸配額。

```
pikpaktui vip
```

---

## completions

產生 Shell 補全腳本，目前僅支援 **Zsh**。

```
pikpaktui completions <shell>
```

**範例：**

```bash
pikpaktui completions zsh
pikpaktui completions zsh > ~/.zfunc/_pikpaktui
eval "$(pikpaktui completions zsh)"
```

詳見 [Shell 補全](/zh-Hant/guide/shell-completions)。
