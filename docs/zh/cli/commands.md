---
title: 命令参考
section: cli
order: 2
locale: zh
---


所有命令均需有效会话，请先运行 `pikpaktui`（TUI）登录，或使用 [`login`](#login)。

---

## ls

列出 PikPak 网盘中的文件和文件夹。

```
pikpaktui ls [选项] [路径]
```

| 参数 | 说明 |
|------|------|
| `-l`, `--long` | 长格式——显示 ID、大小、日期和文件名 |
| `-J`, `--json` | 输出 JSON 数组 |
| `-s`, `--sort <字段>` | 按字段排序：`name`、`size`、`created`、`type`、`extension`、`none` |
| `-r`, `--reverse` | 反向排序 |
| `--tree` | 递归树状视图 |
| `--depth=N` | 限制树深度为 N 层 |

**示例：**

```bash
pikpaktui ls                                # 列出根目录
pikpaktui ls "/My Pack"                    # 列出指定文件夹
pikpaktui ls -l /Movies                    # 长格式
pikpaktui ls --sort=size -r /              # 按大小倒序
pikpaktui ls --tree --depth=2 "/My Pack"   # 树状，最多 2 层
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'
```

---

## mv

将文件或文件夹移动到目标文件夹。

```
pikpaktui mv [选项] <源路径> <目标路径>
pikpaktui mv [选项] -t <目标> <源路径...>
```

| 参数 | 说明 |
|------|------|
| `-t <目标>` | 批量模式——将多个源文件移入目标文件夹 |
| `-n`, `--dry-run` | 预览，不执行 |

**示例：**

```bash
pikpaktui mv "/My Pack/file.txt" /Archive
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt   # 批量
pikpaktui mv -n "/My Pack/a.txt" /Archive        # 预览
```

---

## cp

将文件或文件夹复制到目标文件夹。

```
pikpaktui cp [选项] <源路径> <目标路径>
pikpaktui cp [选项] -t <目标> <源路径...>
```

| 参数 | 说明 |
|------|------|
| `-t <目标>` | 批量模式 |
| `-n`, `--dry-run` | 预览，不执行 |

**示例：**

```bash
pikpaktui cp "/My Pack/file.txt" /Backup
pikpaktui cp -t /Backup /a.txt /b.txt
pikpaktui cp -n -t /Backup /a.txt /b.txt
```

---

## rename

原地重命名文件或文件夹（保持在当前目录）。

```
pikpaktui rename [选项] <路径> <新文件名>
```

| 参数 | 说明 |
|------|------|
| `-n`, `--dry-run` | 预览，不执行 |

**示例：**

```bash
pikpaktui rename "/My Pack/old.txt" new.txt
pikpaktui rename -n "/My Pack/old.txt" new.txt
```

---

## rm

删除文件或文件夹。默认移入回收站，加 `-f` 则永久删除。

```
pikpaktui rm [选项] <路径...>
```

| 参数 | 说明 |
|------|------|
| `-r`, `--recursive` | 删除文件夹时必须加此参数 |
| `-f`, `--force` | 永久删除（跳过回收站） |
| `-rf`, `-fr` | 递归永久删除文件夹 |
| `-n`, `--dry-run` | 预览，不执行 |

**示例：**

```bash
pikpaktui rm "/My Pack/file.txt"             # 移入回收站
pikpaktui rm /a.txt /b.txt /c.txt            # 批量删除
pikpaktui rm -r "/My Pack/folder"            # 文件夹移入回收站
pikpaktui rm -rf "/My Pack/old-folder"       # 永久删除文件夹
pikpaktui rm -n -rf "/My Pack/folder"        # 预览永久删除
```

:::callout[warning]{kind="warn"}
`-f` 会永久删除，无法恢复。建议先用 dry-run 确认。
:::

---

## mkdir

创建文件夹或嵌套路径。

```
pikpaktui mkdir [选项] <父路径> <文件夹名>
pikpaktui mkdir [选项] -p <完整路径>
```

| 参数 | 说明 |
|------|------|
| `-p` | 递归创建 `<完整路径>` 中所有不存在的中间目录 |
| `-n`, `--dry-run` | 预览，不执行 |

**示例：**

```bash
pikpaktui mkdir "/My Pack" NewFolder           # 创建单个文件夹
pikpaktui mkdir -p "/My Pack/a/b/c"            # 递归创建嵌套路径
pikpaktui mkdir -n "/My Pack" NewFolder        # 预览
pikpaktui mkdir -n -p "/My Pack/a/b/c"         # 预览嵌套创建
```

:::callout[tip]{kind="info"}
不带 `-p` 时语法为 `<父路径> <文件夹名>`（两个参数）；带 `-p` 时传完整路径一个参数。
:::

---

## info

显示文件或文件夹的详细元数据，视频文件还会包含媒体轨道信息。

```
pikpaktui info [选项] <路径>
```

| 参数 | 说明 |
|------|------|
| `-J`, `--json` | JSON 输出（含 hash、下载链接、媒体轨道） |

**示例：**

```bash
pikpaktui info "/My Pack/video.mp4"
pikpaktui info "/My Pack/video.mp4" --json
```

---

## link

打印文件的直链下载地址，可选附带视频流地址。

```
pikpaktui link [选项] <路径>
```

| 参数 | 说明 |
|------|------|
| `-m`, `--media` | 同时显示转码视频流地址 |
| `-c`, `--copy` | 复制 URL 到剪贴板 |
| `-J`, `--json` | JSON 输出：`{name, url, size}` |

**示例：**

```bash
pikpaktui link "/My Pack/file.zip"
pikpaktui link "/My Pack/file.zip" --copy
pikpaktui link "/My Pack/video.mp4" -m
pikpaktui link -mc "/My Pack/video.mp4"    # 媒体 + 复制
```

---

## cat

将文本文件内容输出到标准输出，适合预览存在 PikPak 上的小型文本或配置文件。

```
pikpaktui cat <路径>
```

**示例：**

```bash
pikpaktui cat "/My Pack/notes.txt"
```

---

## play

用外部播放器播放视频流。不指定画质则列出可用选项。

```
pikpaktui play <路径> [画质]
```

| 参数 | 说明 |
|------|------|
| `画质` | 流画质：`720`、`1080`、`original`，或按编号选择 |

**示例：**

```bash
pikpaktui play "/My Pack/video.mp4"            # 列出可用流
pikpaktui play "/My Pack/video.mp4" 1080       # 播放 1080p
pikpaktui play "/My Pack/video.mp4" original   # 播放原始文件
pikpaktui play "/My Pack/video.mp4" 2          # 按编号播放第 2 条流
```

:::callout[播放器配置]{kind="info"}
在 `config.toml` 中设置 `player = "mpv"`，或在 TUI 设置面板中配置。支持任意命令行视频播放器：`mpv`、`vlc`、`iina`、`celluloid` 等。
:::

---

## download

下载文件或递归下载整个文件夹到本地。

```
pikpaktui download [选项] <路径>
pikpaktui download [选项] -t <本地目录> <路径...>
```

| 参数 | 说明 |
|------|------|
| `-o`, `--output <文件>` | 自定义输出文件名（仅单文件） |
| `-t <本地目录>` | 批量模式——将多个文件下载到指定目录 |
| `-j`, `--jobs <n>` | 并发下载线程数（默认 1） |
| `-n`, `--dry-run` | 预览，不下载 |

**示例：**

```bash
pikpaktui download "/My Pack/file.txt"                  # 下载到当前目录
pikpaktui download "/My Pack/file.txt" /tmp/file.txt    # 下载到指定路径
pikpaktui download -o output.mp4 "/My Pack/video.mp4"   # 自定义文件名
pikpaktui download "/My Pack/folder"                    # 递归下载文件夹
pikpaktui download -j4 -t ./videos/ /a.mp4 /b.mp4      # 4 并发，批量
pikpaktui download -n "/My Pack/folder"                 # 预览
```

---

## upload

上传本地文件到 PikPak，支持秒传（已存在相同 hash）和断点续传。

```
pikpaktui upload [选项] <本地路径> [远程路径]
pikpaktui upload [选项] -t <远程目录> <本地文件...>
```

| 参数 | 说明 |
|------|------|
| `[远程路径]` | 可选目标文件夹（单文件时位置参数） |
| `-t <远程目录>` | 批量模式——上传多个文件到指定目录 |
| `-n`, `--dry-run` | 预览，不上传 |

**示例：**

```bash
pikpaktui upload ./file.txt                        # 上传到根目录
pikpaktui upload ./file.txt "/My Pack"             # 上传到指定文件夹
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt     # 批量上传
pikpaktui upload -n ./file.txt "/My Pack"          # 预览
```

:::callout[秒传]{kind="info"}
如果 PikPak 服务器上已存在相同 hash 的文件，上传瞬间完成，不传输任何数据。
:::

---

## share

创建、列出、保存和删除分享链接。

```
pikpaktui share [选项] <路径...>      # 创建
pikpaktui share -l                    # 列出我的分享
pikpaktui share -S <URL>              # 保存他人分享到网盘
pikpaktui share -D <ID...>            # 删除分享
```

**创建选项：**

| 参数 | 说明 |
|------|------|
| `-p`, `--password` | 自动生成访问密码 |
| `-d`, `--days <n>` | 有效期（天）；`-1` 表示永久（默认） |
| `-o <文件>` | 将分享 URL 写入文件 |
| `-J`, `--json` | JSON 输出：`{share_id, share_url, pass_code}` |

**保存选项（与 `-S` 配合）：**

| 参数 | 说明 |
|------|------|
| `-p <密码>` | 加密分享的访问码 |
| `-t <路径>` | 保存目标文件夹 |
| `-n`, `--dry-run` | 预览，不保存 |

**示例：**

```bash
pikpaktui share "/My Pack/file.txt"              # 创建普通分享
pikpaktui share -p "/My Pack/file.txt"           # 加密分享
pikpaktui share -d 7 "/My Pack/file.txt"         # 7 天有效期
pikpaktui share -p -d 7 /a.txt /b.txt            # 多文件+加密+7天

pikpaktui share -l                               # 列出我的分享
pikpaktui share -l -J                            # JSON 格式

pikpaktui share -D abc123                        # 删除指定分享
pikpaktui share -D abc123 def456                 # 删除多个

pikpaktui share -S "https://mypikpak.com/s/XXXX"              # 保存到根目录
pikpaktui share -S -p PO -t "/My Pack" "https://..."          # 含密码+指定目标
pikpaktui share -S -n "https://mypikpak.com/s/XXXX"           # 预览
```

---

## offline

提交 URL 或磁力链接进行服务器端（云端）下载，下载在 PikPak 服务器上执行。

```
pikpaktui offline [选项] <URL>
```

| 参数 | 说明 |
|------|------|
| `--to`, `-t <路径>` | PikPak 中的目标文件夹 |
| `--name`, `-n <名称>` | 覆盖任务/文件名 |
| `--dry-run` | 预览，不创建任务 |

**示例：**

```bash
pikpaktui offline "magnet:?xt=urn:btih:abc123..."
pikpaktui offline --to "/Downloads" "https://example.com/file.zip"
pikpaktui offline --to "/Downloads" --name "myvideo.mp4" "https://..."
pikpaktui offline --dry-run "magnet:?xt=..."
```

---

## tasks

管理离线下载任务。

```
pikpaktui tasks [子命令] [选项] [数量]
```

**子命令：**

| 子命令 | 说明 |
|--------|------|
| `list`、`ls` | 列出任务（不指定子命令时的默认行为） |
| `retry <ID>` | 重试失败的任务 |
| `delete <ID...>`、`rm <ID...>` | 删除任务 |

**选项：**

| 参数 | 说明 |
|------|------|
| `-J`, `--json` | JSON 输出（用于 `list`） |
| `-n`, `--dry-run` | 预览（用于 `delete`） |
| `<数字>` | 限制结果数量（默认 50） |

**示例：**

```bash
pikpaktui tasks                              # 列出最多 50 条任务
pikpaktui tasks list 10                     # 列出 10 条
pikpaktui tasks list --json                 # JSON 输出
pikpaktui tasks retry abc12345              # 重试失败任务
pikpaktui tasks delete abc12345             # 删除任务
pikpaktui tasks rm abc12345 def67890        # 删除多个任务
```

---

## trash

列出回收站中的文件。

```
pikpaktui trash [选项] [数量]
```

| 参数 | 说明 |
|------|------|
| `-l`, `--long` | 长格式——显示 ID、大小、日期 |
| `-J`, `--json` | JSON 输出 |
| `<数字>` | 最大结果数（默认 100） |

**示例：**

```bash
pikpaktui trash              # 列出最多 100 条
pikpaktui trash 50           # 最多 50 条
pikpaktui trash -l           # 长格式
pikpaktui trash --json       # JSON 输出
```

---

## untrash

按精确文件名从回收站恢复文件。

```
pikpaktui untrash [选项] <文件名...>
```

| 参数 | 说明 |
|------|------|
| `-n`, `--dry-run` | 预览，不恢复 |

**示例：**

```bash
pikpaktui untrash "file.txt"
pikpaktui untrash "a.txt" "b.mp4"      # 恢复多个
pikpaktui untrash -n "file.txt"        # 预览
```

:::callout[tip]{kind="info"}
按精确文件名匹配，不是路径。若多个已删除文件同名，恢复第一个匹配项。
:::

---

## star

收藏一个或多个文件。

```
pikpaktui star <路径...>
```

**示例：**

```bash
pikpaktui star "/My Pack/video.mp4"
pikpaktui star "/My Pack/a.txt" "/My Pack/b.txt"
```

---

## unstar

取消收藏一个或多个文件。

```
pikpaktui unstar <路径...>
```

**示例：**

```bash
pikpaktui unstar "/My Pack/video.mp4"
```

---

## starred

列出所有收藏的文件。

```
pikpaktui starred [选项] [数量]
```

| 参数 | 说明 |
|------|------|
| `-l`, `--long` | 长格式 |
| `-J`, `--json` | JSON 输出 |
| `<数字>` | 最大结果数（默认 100） |

**示例：**

```bash
pikpaktui starred
pikpaktui starred 50
pikpaktui starred -l
pikpaktui starred --json
```

---

## events

列出最近的文件操作记录（上传、下载、删除等）。

```
pikpaktui events [选项] [数量]
```

| 参数 | 说明 |
|------|------|
| `-J`, `--json` | JSON 输出 |
| `<数字>` | 最大结果数（默认 20） |

**示例：**

```bash
pikpaktui events
pikpaktui events 50
pikpaktui events --json
```

---

## login

登录 PikPak 并将凭据保存到 `~/.config/pikpaktui/login.yaml`。

```
pikpaktui login [选项]
```

| 参数 | 说明 |
|------|------|
| `-u`, `--user <邮箱>` | PikPak 账号邮箱 |
| `-p`, `--password <密码>` | PikPak 账号密码 |

环境变量（优先级低于命令行参数）：

| 变量 | 说明 |
|------|------|
| `PIKPAK_USER` | 账号邮箱 |
| `PIKPAK_PASS` | 账号密码 |

**示例：**

```bash
pikpaktui login                                          # 交互式提示
pikpaktui login -u user@example.com -p mypassword
PIKPAK_USER=user@example.com PIKPAK_PASS=pass pikpaktui login
```

---

## quota

显示存储空间和带宽配额。

```
pikpaktui quota [选项]
```

| 参数 | 说明 |
|------|------|
| `-J`, `--json` | JSON 输出 |

**示例：**

```bash
pikpaktui quota
pikpaktui quota --json
```

---

## vip

显示 VIP 会员状态、邀请码和传输配额。

```
pikpaktui vip
```

---

## update

检查更新并从 GitHub Releases 自动更新二进制文件。

```bash
pikpaktui update
```

下载适合当前平台的最新版本并就地替换当前二进制文件。

---

## completions

生成 Shell 补全脚本，目前仅支持 **Zsh**。

```
pikpaktui completions <shell>
```

**示例：**

```bash
pikpaktui completions zsh                            # 输出到标准输出
pikpaktui completions zsh > ~/.zfunc/_pikpaktui      # 保存到文件
eval "$(pikpaktui completions zsh)"                  # 在当前 shell 中加载
```

详见 [Shell 补全](/zh/guide/shell-completions)。
