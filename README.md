# pikpaktui

`pikpaktui` 是一个 Rust 编写的 PikPak 文件管理 TUI。

当前处于重构阶段：
- TUI 交互已可用（浏览、复制、移动、重命名、删除）
- 后端已抽象为 `Backend` trait
- 默认后端为 `cli`（调用 `pikpakcli`）
- 已引入 `native` 后端骨架与 session 持久化能力（逐步替换中）

运行方式：
- 无参数启动：进入 TUI 文件管理
- 有参数启动：透传给 `pikpakcli`（兼容模式）

## 依赖

- Rust (建议 stable)
- 已可用的 `pikpakcli`（当前默认后端需要在 `PATH` 中）

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

## 后端选择

通过环境变量切换后端：

```bash
# 默认：cli
PIKPAKTUI_BACKEND=cli cargo run

# 实验中：native（目前为骨架，操作接口尚在逐步实现）
PIKPAKTUI_BACKEND=native cargo run
```

## TUI 键位

默认路径：`/My Pack`

- `j` / `↓`：下移
- `k` / `↑`：上移
- `Enter`：进入目录（`size=0` 视为目录）
- `Backspace`：返回上级
- `r`：刷新（执行 `pikpakcli ls -l -p <path>`）
- `c`：复制（输入目标路径）
- `m`：移动（输入目标路径）
- `n`：重命名（输入新名字）
- `d`：删除（默认回收站，带二次确认）
- `q`：退出

## smoke test（native auth/session）

可运行以下命令验证 session 持久化 roundtrip：

```bash
cargo run -- --smoke-auth
```

成功时会输出 `smoke-auth-ok ...`。

可运行以下命令验证 native 登录流程最小调用（本地 mock auth server）：

```bash
cargo run -- --smoke-native-login
```

成功时会输出 `smoke-native-login-ok ...`。

## 参数兼容策略（passthrough）

当 `pikpaktui` 启动时带任意参数，不进入 TUI，等价于：

```bash
pikpakcli <原样参数>
```

例如：

```bash
pikpaktui ls -p "/My Pack"
pikpaktui rm -p "/My Pack" --name "foo.txt"
```

输出和退出码会直接继承 `pikpakcli`。

## 代码结构

- `src/main.rs`：参数分流、后端选择（`PIKPAKTUI_BACKEND`）
- `src/backend.rs`：后端抽象（`Backend` trait + `Entry`）
- `src/tui.rs`：界面、事件循环、键位与交互弹窗
- `src/pikpak.rs`：`CliBackend`（`pikpakcli` 子进程实现）
- `src/native/`：`NativeBackend` 与 auth/session 骨架

## 说明

当前重点是把 `native` 后端逐步补齐（auth 登录流程、ls/mv/rename/remove），并保持 TUI 体验连续可用。
