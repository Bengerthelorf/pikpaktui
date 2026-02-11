# pikpaktui

Rust 编写的 PikPak 文件管理工具，支持 TUI 交互界面和 CLI 子命令，纯 Rust 实现，无外部运行时依赖。

## 功能

- TUI 文件浏览器：进入目录、返回上级、breadcrumb 导航
- CLI 子命令：ls / mv / cp / rename / rm / mkdir / download / quota
- 移动 / 复制 / 重命名 / 删除（回收站）/ 新建文件夹
- 文件下载（支持断点续传）
- 配额查询
- TUI 内登录表单，登录后自动保存凭据
- session 持久化，重启免登录

## 安装与运行

```bash
cargo build --release
./target/release/pikpaktui
```

或直接：

```bash
cargo run
```

## CLI 子命令

```bash
pikpaktui ls /                           # 列出根目录
pikpaktui ls "/My Pack"                  # 列出 My Pack
pikpaktui mv "/My Pack/file.txt" /Archive  # 移动文件
pikpaktui cp "/My Pack/file.txt" /Backup   # 复制文件
pikpaktui rename "/My Pack/old.txt" new.txt  # 重命名
pikpaktui rm "/My Pack/file.txt"           # 删除（回收站）
pikpaktui mkdir "/My Pack" newfolder       # 创建文件夹
pikpaktui download "/My Pack/file.txt"     # 下载到当前目录
pikpaktui download "/My Pack/file.txt" /tmp/file.txt  # 下载到指定路径
pikpaktui quota                            # 显示配额
```

CLI 模式需要登录：先查 session，再查 config.yaml，都没有则提示用 TUI 登录。

## 登录方式

### 1) TUI 登录表单（推荐）

直接启动 `pikpaktui`，如果没有有效 session，会显示登录表单。

- `Tab`：在 Email / Password 之间切换
- `Enter`：提交登录
- `Esc`：退出

登录成功后，凭据自动保存到 `config.yaml`，session 写入 `session.json`。

### 2) config.yaml

手动创建配置文件：

- macOS: `~/Library/Application Support/pikpaktui/config.yaml`
- Linux: `~/.config/pikpaktui/config.yaml`

```yaml
username: "you@example.com"
password: "your-password"
```

启动时如果没有有效 session，会自动读取 config.yaml 尝试登录。

## TUI 键位

| 按键 | 操作 |
|------|------|
| `j` / `↓` | 下移 |
| `k` / `↑` | 上移 |
| `Enter` | 进入目录 |
| `Backspace` | 返回上级 |
| `r` | 刷新 |
| `c` | 复制（输入目标路径） |
| `m` | 移动（输入目标路径） |
| `n` | 重命名（输入新名字） |
| `d` | 删除（回收站，二次确认） |
| `f` | 新建文件夹 |
| `q` | 退出 |

## 代码结构

- `src/main.rs` — 入口、CLI 子命令分发
- `src/config.rs` — config.yaml 读写
- `src/pikpak.rs` — PikPak API client（认证、文件操作、下载）
- `src/tui.rs` — TUI 界面与交互（ID-based 导航、登录表单）
