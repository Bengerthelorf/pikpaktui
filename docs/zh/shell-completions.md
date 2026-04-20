---
title: Shell 补全
section: guide
order: 4
locale: zh
---


pikpaktui 提供动态 Shell 补全，可以**实时从你的 PikPak 网盘补全云端路径**——类似 `scp` 补全远程路径的体验。

## Zsh

:::code-group

```bash [eval（最简单）]
# 添加到 ~/.zshrc
eval "$(pikpaktui completions zsh)"
```

```bash [fpath（启动更快）]
# 生成一次并保存到 fpath
pikpaktui completions zsh > ~/.zfunc/_pikpaktui

# 在 ~/.zshrc 中添加（需在 compinit 之前）：
fpath=(~/.zfunc $fpath)
autoload -Uz compinit
compinit
```

```bash [Oh My Zsh]
pikpaktui completions zsh > \
  ${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/completions/_pikpaktui
# 重启 shell 或运行：omz reload
```

:::

:::callout[fzf-tab]{kind="info"}
搭配 [fzf-tab](https://github.com/Aloxaf/fzf-tab) 使用效果更佳——输入路径前缀后按 Tab 会弹出可模糊搜索的云端文件列表。
:::

## 补全覆盖范围

| 场景 | 补全内容 |
|------|---------|
| `pikpaktui <Tab>` | 所有子命令及说明 |
| `pikpaktui ls /<Tab>` | 实时列出云端目录 |
| `pikpaktui ls -<Tab>` | `-l`、`--long`、`-J`、`--json`、`-s`、`--sort`、`-r`、`--reverse`、`--tree`、`--depth` |
| `pikpaktui ls --sort <Tab>` | `name`、`size`、`created`、`type`、`extension`、`none` |
| `pikpaktui mv /src<Tab>` | 云端路径补全 |
| `pikpaktui mv -t /dst<Tab>` | `-t` 目标的云端路径 |
| `pikpaktui cp /src<Tab>` | 云端路径补全 |
| `pikpaktui download /cloud<Tab>` | 云端路径补全 |
| `pikpaktui download -o <Tab>` | 本地文件路径 |
| `pikpaktui upload <Tab>` | 本地文件路径 |
| `pikpaktui upload -t /dst<Tab>` | `-t` 目标的云端路径 |
| `pikpaktui share /path<Tab>` | 云端路径补全 |
| `pikpaktui offline --to /dst<Tab>` | `--to` 目标的云端路径 |
| `pikpaktui tasks <Tab>` | `list`、`ls`、`retry`、`delete`、`rm` |
| `pikpaktui rm -<Tab>` | `-r`、`-f`、`-rf`、`-fr` |
| `pikpaktui mkdir -<Tab>` | `-p` |
| `pikpaktui info /path<Tab>` | 云端路径 |
| `pikpaktui cat /path<Tab>` | 云端路径 |
| `pikpaktui play /path<Tab>` | 云端路径 |
| `pikpaktui rename /path<Tab>` | 云端路径 |
| `pikpaktui star /path<Tab>` | 云端路径 |
| `pikpaktui unstar /path<Tab>` | 云端路径 |
| `pikpaktui completions <Tab>` | `zsh` |

## 云端路径补全原理

输入云端路径前缀后按 `Tab`，pikpaktui 会：

1. 在内部调用 `pikpaktui __complete_path <dir>`
2. 列出该远程目录的内容
3. 返回条目名，文件夹名带尾部 `/`（再次按 Tab 可继续深入）

前提：
- 有效的会话文件（`~/.config/pikpaktui/session.json`）
- 可访问 PikPak API

首次补全可能有短暂延迟，同目录的后续补全速度很快。

## 支持的 Shell

目前仅支持 **Zsh**，暂不支持 Fish 和 Bash。
