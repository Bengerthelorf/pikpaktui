# pikpaktui

`pikpaktui` 是一个 Rust 编写的 PikPak 文件管理 TUI。

当前版本已经默认使用 **纯 Rust native backend**，不再依赖 `pikpakcli` 运行时。

## 当前能力

- native auth：`captcha/init + signin + session 持久化`
- native ls
- native move
- native rename
- native remove（回收站语义）
- native copy

## 依赖

- Rust (stable)
- 无 `pikpakcli` 运行时依赖

## 安装与运行

```bash
cd /Users/snaix/Documents/pikpaktui
cargo run
```

构建后：

```bash
cargo build --release
./target/release/pikpaktui
```

## 登录（native）

### 1) 环境变量

```bash
export PIKPAK_EMAIL='you@example.com'
export PIKPAK_PASSWORD='***'
```

可选：

```bash
# 当 captcha init 响应不直接返回 token 时，先完成 challenge 再注入
export PIKPAK_CAPTCHA_TOKEN='***'
```

### 2) 执行登录

```bash
cargo run -- --native-login
```

成功后会写入 session 文件（默认在系统 config 目录下）。

## TUI 键位

默认路径：`/My Pack`

- `j` / `↓`：下移
- `k` / `↑`：上移
- `Enter`：进入目录（`size=0` 视为目录）
- `Backspace`：返回上级
- `r`：刷新
- `c`：复制（输入目标路径）
- `m`：移动（输入目标路径）
- `n`：重命名（输入新名字）
- `d`：删除（回收站，二次确认）
- `q`：退出

## smoke tests

```bash
cargo run -- --smoke-auth
cargo run -- --smoke-native-login
cargo run -- --smoke-native-ls
cargo run -- --smoke-native-ops
```

成功会分别输出：

- `smoke-auth-ok ...`
- `smoke-native-login-ok ...`
- `smoke-native-ls-ok ...`
- `smoke-native-ops-ok`

## 代码结构

- `src/main.rs`：入口、smoke 命令、native login 命令
- `src/backend.rs`：后端抽象（`Backend` trait + `Entry`）
- `src/tui.rs`：界面与交互
- `src/native/auth.rs`：captcha/init + signin + session
- `src/native/mod.rs`：native drive API（ls/mv/cp/rename/remove）
