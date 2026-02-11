# pikpaktui

`pikpaktui` 是一个基于 `pikpakcli` 的轻量 TUI 封装（MVP），目标是先把常用浏览/管理流程跑通。

- 无参数启动：进入 TUI 文件管理
- 有参数启动：直接透传给 `pikpakcli`（wrapper 模式）

## 依赖

- Rust (建议 stable)
- 已可用的 `pikpakcli`（需要在 `PATH` 中）

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

## TUI 键位

默认路径：`/My Pack`

- `j` / `↓`：下移
- `k` / `↑`：上移
- `Enter`：进入目录（`size=0` 视为目录）
- `Backspace`：返回上级
- `r`：刷新（执行 `pikpakcli ls -l -p <path>`）
- `m`：移动（输入目标路径）
- `n`：重命名（输入新名字）
- `d`：删除（默认回收站，带二次确认）
- `q`：退出

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

- `src/main.rs`：参数分流（TUI / passthrough）
- `src/tui.rs`：界面、事件循环、键位与交互弹窗
- `src/pikpak.rs`：`pikpakcli` 子进程调用封装（`ls/mv/rename/remove`）

## 说明

当前是 MVP：优先可用与清晰结构，后续可继续增强（更稳健的 `ls` 解析、命令参数适配、错误提示等）。
