# pikpaktui

Rust 编写的 PikPak 文件管理 TUI，纯 Rust native backend，无外部运行时依赖。

## 功能

- 文件浏览、进入目录、返回上级
- 移动 / 复制 / 重命名 / 删除（回收站）
- TUI 内登录表单，登录后自动保存凭据
- 支持 `config.yaml` 配置账号密码
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

### 3) 命令行登录（CI / 脚本）

```bash
export PIKPAK_EMAIL='you@example.com'
export PIKPAK_PASSWORD='***'
cargo run -- --native-login
```

## TUI 键位

默认路径：`/My Pack`

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
| `q` | 退出 |

## Smoke Tests

```bash
cargo run -- --smoke-auth
cargo run -- --smoke-native-login
cargo run -- --smoke-native-ls
cargo run -- --smoke-native-ops
```

## 代码结构

- `src/main.rs` — 入口、CLI 命令、启动流程
- `src/config.rs` — config.yaml 读写
- `src/backend.rs` — Backend trait + Entry
- `src/tui.rs` — TUI 界面与交互（含登录表单）
- `src/native/auth.rs` — captcha/init + signin + session
- `src/native/mod.rs` — native drive API（ls/mv/cp/rename/remove）
