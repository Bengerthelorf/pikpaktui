# 快速开始

pikpaktui 是一个 [PikPak](https://mypikpak.com) 云存储的终端客户端，提供交互式 TUI 和包含 26 条子命令的完整 CLI。由纯 Rust 编写，无 OpenSSL，无 C 依赖。

## 系统要求

- PikPak 账号（[立即注册](https://mypikpak.com)）
- macOS（Intel 或 Apple Silicon）、Linux（x86_64 或 ARM64）、Windows（x86_64 或 ARM64）或 FreeBSD

## 安装

::: code-group

```bash [安装脚本]
curl -fsSL https://app.snaix.homes/pikpaktui/install | bash
```

```bash [Homebrew]
brew install Bengerthelorf/tap/pikpaktui
```

```bash [Cargo]
cargo install pikpaktui
```

```bash [从源码构建]
git clone https://github.com/Bengerthelorf/pikpaktui.git
cd pikpaktui
cargo build --release
# 二进制文件位于 ./target/release/pikpaktui
```

:::

预编译二进制（包含 Linux musl 静态版）可在 [Releases 页面](https://github.com/Bengerthelorf/pikpaktui/releases/latest)下载。

## 首次启动——TUI

不带参数运行即可打开交互式 TUI：

```bash
pikpaktui
```

首次运行会显示登录表单，输入 PikPak 邮箱和密码。登录成功后，凭据保存至 `~/.config/pikpaktui/login.yaml`，会话保存至 `~/.config/pikpaktui/session.json`，之后无需再次登录（会自动刷新）。

![TUI 主界面](/images/main.jpeg)

::: tip 快捷键速查
- `,` — 打开设置
- `h` — 显示完整快捷键帮助
- `q` — 退出
:::

## TUI 导航

TUI 是三列 Miller 式布局：

| 面板 | 内容 |
|------|------|
| **左侧** | 上级目录 |
| **中间** | 当前目录（主操作区） |
| **右侧** | 预览（缩略图 / 文本 / 子目录列表） |

### 基本导航

| 按键 | 操作 |
|------|------|
| `j` / `k` 或 `↑` / `↓` | 上下移动光标 |
| `Enter` | 进入文件夹 / 播放视频 |
| `Backspace` | 返回上级目录 |
| `g` / `Home` | 跳到顶部 |
| `G` / `End` | 跳到底部 |
| `Ctrl+U` / `Ctrl+D` | 半页滚动 |
| `r` | 刷新当前目录 |
| `:` | 输入路径跳转 |

### 文件操作

| 按键 | 操作 |
|------|------|
| `m` | 移动（文件夹选择器或文本输入，取决于 `move_mode` 配置） |
| `c` | 复制 |
| `n` | 重命名（内联文本输入） |
| `d` | 删除（提示确认：`y` 移入回收站，`p` 永久删除） |
| `f` | 新建文件夹 |
| `s` | 收藏 / 取消收藏 |
| `y` | 复制直链地址到剪贴板（仅文件） |

### 视图与功能

| 按键 | 操作 |
|------|------|
| `a` | 将当前文件加入/移出购物车 |
| `A` | 打开购物车（批量下载/移动/复制） |
| `u` | 上传本地文件到当前文件夹 |
| `w` | 在线播放视频——打开画质选择器 |
| `o` | 离线下载——输入 URL 或磁力链接 |
| `O` | 离线任务视图 |
| `D` | 下载管理视图 |
| `t` | 回收站视图 |
| `M` | 我的分享视图 |
| `Space` | 文件/文件夹详情弹窗 |
| `l` | 切换日志面板 |

详见 [TUI 指南](/zh/guide/tui)。

## CLI 快速上手

所有 CLI 子命令均需要有效会话（先运行 `pikpaktui` 登录，或使用 `pikpaktui login`）。

```bash
# 列出文件
pikpaktui ls /
pikpaktui ls -l "/My Pack"        # 长格式（含大小和日期）
pikpaktui ls --tree --depth=2 /   # 递归树状视图

# 文件操作
pikpaktui mv "/My Pack/file.txt" /Archive
pikpaktui cp "/My Pack/video.mp4" /Backup
pikpaktui rename "/My Pack/old.txt" new.txt
pikpaktui rm "/My Pack/file.txt"           # 移入回收站
pikpaktui rm -rf "/My Pack/old-folder"    # 永久删除

# 传输
pikpaktui download "/My Pack/video.mp4"
pikpaktui download -j4 -t ./videos/ /a.mp4 /b.mp4  # 4 并发
pikpaktui upload ./notes.txt "/My Pack"

# 离线下载
pikpaktui offline "magnet:?xt=urn:btih:..."
pikpaktui offline --to "/Downloads" "https://example.com/file.zip"
```

::: tip Dry run 预览
所有修改数据的命令都支持 `-n` / `--dry-run`——会解析路径并展示操作计划，不做任何实际修改。
:::

## 通过 CLI 登录

适合脚本或 CI 场景的非交互式登录：

```bash
pikpaktui login -u you@example.com -p yourpassword

# 也可通过环境变量：
PIKPAK_USER=you@example.com PIKPAK_PASS=yourpassword pikpaktui login
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `PIKPAK_USER` | 账号邮箱（`login` 命令使用） |
| `PIKPAK_PASS` | 账号密码（`login` 命令使用） |
| `PIKPAK_DRIVE_BASE_URL` | 覆盖 PikPak Drive API 地址 |
| `PIKPAK_AUTH_BASE_URL` | 覆盖 PikPak 认证 API 地址 |
| `PIKPAK_CLIENT_ID` | 覆盖 OAuth Client ID |
| `PIKPAK_CLIENT_SECRET` | 覆盖 OAuth Client Secret |
| `PIKPAK_CAPTCHA_TOKEN` | 登录遭遇验证码时提供 token |

## 下一步

- [TUI 指南](/zh/guide/tui) — 所有视图的完整快捷键参考
- [配置](/zh/guide/configuration) — 自定义配色、字体、播放器等
- [CLI 命令参考](/zh/cli/commands) — 全部 27 条命令详解
- [Shell 补全](/zh/guide/shell-completions) — 在 zsh 中 Tab 补全云端路径
