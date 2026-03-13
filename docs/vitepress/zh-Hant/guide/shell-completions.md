# Shell 補全

pikpaktui 提供動態 Shell 補全，可以**即時從你的 PikPak 網盤補全雲端路徑**——類似 `scp` 補全遠端路徑的體驗。

## Zsh

::: code-group

```bash [eval（最簡單）]
# 加入 ~/.zshrc
eval "$(pikpaktui completions zsh)"
```

```bash [fpath（啟動更快）]
# 產生一次並儲存至 fpath
pikpaktui completions zsh > ~/.zfunc/_pikpaktui

# 在 ~/.zshrc 中加入（需在 compinit 之前）：
fpath=(~/.zfunc $fpath)
autoload -Uz compinit
compinit
```

```bash [Oh My Zsh]
pikpaktui completions zsh > \
  ${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/completions/_pikpaktui
# 重新啟動 shell 或執行：omz reload
```

:::

::: tip fzf-tab
搭配 [fzf-tab](https://github.com/Aloxaf/fzf-tab) 使用效果更佳——輸入路徑前綴後按 Tab 會彈出可模糊搜尋的雲端檔案清單。
:::

## 補全涵蓋範圍

| 情境 | 補全內容 |
|------|---------|
| `pikpaktui <Tab>` | 所有子指令及說明 |
| `pikpaktui ls /<Tab>` | 即時列出雲端目錄 |
| `pikpaktui ls -<Tab>` | `-l`、`--long`、`-J`、`--json`、`-s`、`--sort`、`-r`、`--reverse`、`--tree`、`--depth` |
| `pikpaktui ls --sort <Tab>` | `name`、`size`、`created`、`type`、`extension`、`none` |
| `pikpaktui mv /src<Tab>` | 雲端路徑補全 |
| `pikpaktui mv -t /dst<Tab>` | `-t` 目標的雲端路徑 |
| `pikpaktui download /cloud<Tab>` | 雲端路徑補全 |
| `pikpaktui download -o <Tab>` | 本機檔案路徑 |
| `pikpaktui upload <Tab>` | 本機檔案路徑 |
| `pikpaktui upload -t /dst<Tab>` | `-t` 目標的雲端路徑 |
| `pikpaktui share /path<Tab>` | 雲端路徑補全 |
| `pikpaktui offline --to /dst<Tab>` | `--to` 目標的雲端路徑 |
| `pikpaktui tasks <Tab>` | `list`、`ls`、`retry`、`delete`、`rm` |
| `pikpaktui rm -<Tab>` | `-r`、`-f`、`-rf`、`-fr` |
| `pikpaktui mkdir -<Tab>` | `-p` |
| `pikpaktui info /path<Tab>` | 雲端路徑 |
| `pikpaktui cat /path<Tab>` | 雲端路徑 |
| `pikpaktui play /path<Tab>` | 雲端路徑 |
| `pikpaktui rename /path<Tab>` | 雲端路徑 |
| `pikpaktui completions <Tab>` | `zsh` |

## 雲端路徑補全原理

輸入雲端路徑前綴後按 `Tab`，pikpaktui 會：

1. 在內部呼叫 `pikpaktui __complete_path <dir>`
2. 列出該遠端目錄的內容
3. 回傳項目名稱，資料夾名稱帶尾部 `/`（再按 Tab 可繼續深入）

前提條件：
- 有效的工作階段檔案（`~/.config/pikpaktui/session.json`）
- 可存取 PikPak API

首次補全可能有短暫延遲，同目錄的後續補全速度很快。

## 支援的 Shell

目前僅支援 **Zsh**，尚不支援 Fish 和 Bash。
