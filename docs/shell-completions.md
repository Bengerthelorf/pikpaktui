# Shell Completions

## Zsh

Supports dynamic cloud path completion — press `Tab` to list remote files/folders, like `scp`. Works with [fzf-tab](https://github.com/Aloxaf/fzf-tab).

```bash
# Option 1: Add to .zshrc
eval "$(pikpaktui completions zsh)"

# Option 2: Save to fpath
pikpaktui completions zsh > ~/.zfunc/_pikpaktui
# Then in .zshrc: fpath=(~/.zfunc $fpath); autoload -Uz compinit; compinit

# Option 3: Oh My Zsh
pikpaktui completions zsh > ${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/completions/_pikpaktui
```

## What Gets Completed

| Context | Completion |
|---------|------------|
| `pikpaktui <Tab>` | Subcommands with descriptions |
| `pikpaktui ls /<Tab>` | Remote directory listing |
| `pikpaktui ls -<Tab>` | `-l`, `--long`, `-J`, `--json`, `-s`, `--sort`, `-r`, `--reverse`, `--tree`, `--depth` |
| `pikpaktui ls --sort <Tab>` | `name`, `size`, `created`, `type`, `extension`, `none` |
| `pikpaktui mv -<Tab>` | `-t` flag |
| `pikpaktui mv /src<Tab> /dst<Tab>` | Cloud paths for both arguments |
| `pikpaktui download /cloud<Tab> ./<Tab>` | Cloud path, then local path |
| `pikpaktui upload -<Tab>` | `-t` flag |
| `pikpaktui upload ./<Tab> /<Tab>` | Local path, then cloud path |
| `pikpaktui tasks <Tab>` | `list`, `retry`, `delete` subcommands |
| `pikpaktui rm -<Tab>` | `-r`, `-f`, `-rf`, `-fr` |
| `pikpaktui mkdir -<Tab>` | `-p` flag |
| `pikpaktui download -<Tab>` | `-o` flag |
| `pikpaktui info /path<Tab>` | Cloud path completion |
| `pikpaktui cat /path<Tab>` | Cloud path completion |
| `pikpaktui play /path<Tab>` | Cloud path completion |
