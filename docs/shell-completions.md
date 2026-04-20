---
title: Shell Completions
section: guide
order: 4
---


pikpaktui generates dynamic shell completions that complete **cloud paths live from your PikPak drive** — similar to how `scp` completes remote paths.

## Zsh

:::code-group

```bash [eval (simplest)]
# Add to ~/.zshrc
eval "$(pikpaktui completions zsh)"
```

```bash [fpath (faster startup)]
# Generate once and save to fpath
pikpaktui completions zsh > ~/.zfunc/_pikpaktui

# Add to ~/.zshrc (before compinit):
fpath=(~/.zfunc $fpath)
autoload -Uz compinit
compinit
```

```bash [Oh My Zsh]
pikpaktui completions zsh > \
  ${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/completions/_pikpaktui
# Restart your shell or run: omz reload
```

:::

:::callout[fzf-tab]{kind="info"}
Cloud path completions work beautifully with [fzf-tab](https://github.com/Aloxaf/fzf-tab) — you get a fuzzy-searchable popup of your remote files as you type.
:::

## What Gets Completed

| Context | Completions |
|---------|-------------|
| `pikpaktui <Tab>` | All subcommands with descriptions |
| `pikpaktui ls /<Tab>` | Live remote directory listing |
| `pikpaktui ls -<Tab>` | `-l`, `--long`, `-J`, `--json`, `-s`, `--sort`, `-r`, `--reverse`, `--tree`, `--depth` |
| `pikpaktui ls --sort <Tab>` | `name`, `size`, `created`, `type`, `extension`, `none` |
| `pikpaktui mv /src<Tab>` | Cloud path completion |
| `pikpaktui mv -t /dst<Tab>` | Cloud path for `-t` target |
| `pikpaktui cp /src<Tab>` | Cloud path completion |
| `pikpaktui download /cloud<Tab>` | Cloud path completion |
| `pikpaktui download -o <Tab>` | Local file path |
| `pikpaktui upload <Tab>` | Local file path |
| `pikpaktui upload -t /dst<Tab>` | Cloud path for `-t` target |
| `pikpaktui share /path<Tab>` | Cloud path completion |
| `pikpaktui offline --to /dst<Tab>` | Cloud path for `--to` |
| `pikpaktui offline --name <Tab>` | (free text) |
| `pikpaktui tasks <Tab>` | `list`, `ls`, `retry`, `delete`, `rm` |
| `pikpaktui rm -<Tab>` | `-r`, `-f`, `-rf`, `-fr` |
| `pikpaktui mkdir -<Tab>` | `-p` |
| `pikpaktui info /path<Tab>` | Cloud path |
| `pikpaktui cat /path<Tab>` | Cloud path |
| `pikpaktui play /path<Tab>` | Cloud path |
| `pikpaktui rename /path<Tab>` | Cloud path |
| `pikpaktui star /path<Tab>` | Cloud path |
| `pikpaktui unstar /path<Tab>` | Cloud path |
| `pikpaktui completions <Tab>` | `zsh` |

## How Cloud Path Completion Works

When you type a cloud path prefix and press `Tab`, pikpaktui:

1. Calls `pikpaktui __complete_path <dir>` internally
2. Lists entries in that remote directory
3. Returns folder names with a trailing `/` (so pressing Tab again descends into them)

Requirements:
- An active session (`~/.config/pikpaktui/session.json`)
- Network access to PikPak API

There may be a brief delay on first completion while the API is queried. Subsequent completions in the same directory are fast.

## Currently Supported Shells

Only **Zsh** is supported at this time. Fish and Bash completions are not yet available.
