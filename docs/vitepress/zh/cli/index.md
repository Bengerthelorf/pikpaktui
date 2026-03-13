# CLI 概览

pikpaktui 提供 26 条 CLI 子命令，适合脚本、自动化和进阶用户。所有命令均需有效会话——先运行 `pikpaktui`（TUI）登录，或使用 `pikpaktui login`。

## 命令分组

### 文件管理

| 命令 | 说明 |
|------|------|
| [`ls`](/zh/cli/commands#ls) | 列出文件和文件夹 |
| [`mv`](/zh/cli/commands#mv) | 移动文件或文件夹 |
| [`cp`](/zh/cli/commands#cp) | 复制文件或文件夹 |
| [`rename`](/zh/cli/commands#rename) | 重命名文件或文件夹 |
| [`rm`](/zh/cli/commands#rm) | 删除到回收站（`-f` 永久删除） |
| [`mkdir`](/zh/cli/commands#mkdir) | 创建文件夹 |
| [`info`](/zh/cli/commands#info) | 查看文件/文件夹详细元数据 |
| [`link`](/zh/cli/commands#link) | 获取直链地址 |
| [`cat`](/zh/cli/commands#cat) | 预览文本文件内容 |

### 播放

| 命令 | 说明 |
|------|------|
| [`play`](/zh/cli/commands#play) | 用外部播放器在线播放视频 |

### 传输

| 命令 | 说明 |
|------|------|
| [`download`](/zh/cli/commands#download) | 下载文件或文件夹 |
| [`upload`](/zh/cli/commands#upload) | 上传文件到 PikPak |
| [`share`](/zh/cli/commands#share) | 创建、列出、保存或删除分享链接 |

### 离线下载

| 命令 | 说明 |
|------|------|
| [`offline`](/zh/cli/commands#offline) | 提交 URL 或磁力链接进行云端下载 |
| [`tasks`](/zh/cli/commands#tasks) | 管理离线下载任务 |

### 回收站

| 命令 | 说明 |
|------|------|
| [`trash`](/zh/cli/commands#trash) | 列出回收站中的文件 |
| [`untrash`](/zh/cli/commands#untrash) | 按文件名从回收站恢复文件 |

### 收藏与动态

| 命令 | 说明 |
|------|------|
| [`star`](/zh/cli/commands#star) | 收藏文件 |
| [`unstar`](/zh/cli/commands#unstar) | 取消收藏 |
| [`starred`](/zh/cli/commands#starred) | 列出收藏的文件 |
| [`events`](/zh/cli/commands#events) | 最近文件操作记录 |

### 认证

| 命令 | 说明 |
|------|------|
| [`login`](/zh/cli/commands#login) | 登录并保存凭据 |

### 账户

| 命令 | 说明 |
|------|------|
| [`quota`](/zh/cli/commands#quota) | 存储空间和带宽配额 |
| [`vip`](/zh/cli/commands#vip) | VIP 状态和账户信息 |

### 工具

| 命令 | 说明 |
|------|------|
| [`completions`](/zh/cli/commands#completions) | 生成 Shell 补全脚本 |

## 常用参数

### JSON 输出

大多数列表类命令支持 `-J` / `--json`，输出机器可读格式，便于管道传给 `jq`：

```bash
pikpaktui ls /Movies --json | jq '.[] | select(.size > 1073741824)'
pikpaktui info "/My Pack/video.mp4" --json
pikpaktui quota --json
```

### Dry run 预览

所有修改数据的命令均支持 `-n` / `--dry-run`，解析路径后打印操作计划，不做实际修改：

```bash
pikpaktui rm -n "/My Pack/file.txt"
pikpaktui mv -n "/My Pack/a.txt" /Archive
pikpaktui download -n "/My Pack/folder"
pikpaktui upload -n ./file.txt "/My Pack"
```

### 批量模式（`-t`）

`mv`、`cp`、`download`、`upload` 支持 `-t <目标>` 对多个文件批量操作：

```bash
pikpaktui mv -t /Archive /a.txt /b.txt /c.txt
pikpaktui download -t ./local/ /a.mp4 /b.mp4
pikpaktui upload -t "/My Pack" ./a.txt ./b.txt
```
