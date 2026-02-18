use anyhow::{Result, anyhow};

const ZSH_COMPLETION: &str = r##"#compdef pikpaktui

# Zsh completion for pikpaktui - PikPak cloud storage CLI/TUI
# Install: eval "$(pikpaktui completions zsh)"
# Or:      pikpaktui completions zsh > ~/.zfunc/_pikpaktui

# Dynamic cloud path completion (like scp remote path completion)
_pikpaktui_cloud_path() {
    local cur="${words[CURRENT]}"
    # Use the same binary the user is invoking
    local bin="${words[1]}"

    # Determine the parent directory to list and the typed prefix
    local dir partial
    if [[ -z "$cur" ]] || [[ "$cur" == "/" ]]; then
        dir="/"
        partial=""
    elif [[ "$cur" == */ ]]; then
        dir="$cur"
        partial=""
    else
        # /foo/bar → dir=/foo/ partial=bar
        if [[ "$cur" == */* ]]; then
            dir="${cur%/*}/"
            partial="${cur##*/}"
        else
            dir="/"
            partial="$cur"
        fi
    fi

    # Normalise: // → /
    [[ "$dir" == "//" ]] && dir="/"

    # Query remote listing (suppress errors, timeout via background + wait)
    local -a raw_entries
    raw_entries=("${(@f)$($bin __complete_path "$dir" 2>/dev/null)}")

    # compadd -p sets a display prefix (the directory part)
    # We only pass names so zsh can filter by $partial
    local -a dirs_arr files_arr
    for entry in "${raw_entries[@]}"; do
        [[ -z "$entry" ]] && continue
        if [[ "$entry" == */ ]]; then
            dirs_arr+=("${entry%/}")
        else
            files_arr+=("$entry")
        fi
    done

    local display_prefix="$dir"
    # For root, prefix is /
    [[ "$display_prefix" == "/" ]] || display_prefix="${display_prefix%/}/"

    # Directories: -S / appends slash, -q removes it if user types more
    (( ${#dirs_arr} )) && compadd -p "$display_prefix" -S '/' -q -- "${dirs_arr[@]}"
    # Files: normal space suffix
    (( ${#files_arr} )) && compadd -p "$display_prefix" -- "${files_arr[@]}"

    return 0
}

_pikpaktui() {
    local -a commands
    commands=(
        'ls:List files (colored grid; -l for long)'
        'mv:Move file(s) (-t for batch)'
        'cp:Copy file(s) (-t for batch)'
        'rename:Rename a file or folder'
        'rm:Remove to trash (-r folder, -f permanent)'
        'mkdir:Create folder (-p recursive)'
        'download:Download a file (-o output path)'
        'upload:Upload file(s) (-t for batch)'
        'share:Share file(s) as PikPak links'
        'offline:Cloud download a URL or magnet link'
        'tasks:Manage offline download tasks'
        'star:Star files'
        'unstar:Unstar files'
        'starred:List starred files'
        'events:Recent file events'
        'trash:List trashed files'
        'untrash:Restore files from trash'
        'info:Show detailed file/folder info'
        'cat:Preview text file contents'
        'play:Play video with external player'
        'quota:Show storage quota'
        'vip:Show VIP & account info'
        'completions:Generate shell completions'
        'help:Show help message'
        'version:Show version'
    )

    if (( CURRENT == 2 )); then
        _describe -t commands 'pikpaktui command' commands
        return
    fi

    local cmd="${words[2]}"
    case "$cmd" in
        ls)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-l' '--long' '-s' '--sort' '-r' '--reverse'
            elif [[ "${words[CURRENT-1]}" == "-s" ]] || [[ "${words[CURRENT-1]}" == "--sort" ]]; then
                compadd -- 'name' 'size' 'created' 'type' 'extension' 'none'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        mv|cp)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-t'
            elif [[ "${words[CURRENT-1]}" == "-t" ]]; then
                _pikpaktui_cloud_path
            else
                _pikpaktui_cloud_path
            fi
            ;;
        rename)
            if (( CURRENT == 3 )); then
                _pikpaktui_cloud_path
            fi
            ;;
        rm)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-r' '-f' '-rf' '-fr'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        mkdir)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-p'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        download)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-o'
            elif [[ "${words[CURRENT-1]}" == "-o" ]]; then
                _files
            else
                _pikpaktui_cloud_path
            fi
            ;;
        upload)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-t'
            elif [[ "${words[CURRENT-1]}" == "-t" ]]; then
                _pikpaktui_cloud_path
            else
                _files
            fi
            ;;
        share)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-o'
            elif [[ "${words[CURRENT-1]}" == "-o" ]]; then
                _files
            else
                _pikpaktui_cloud_path
            fi
            ;;
        offline)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-t' '--to' '-n' '--name'
            elif [[ "${words[CURRENT-1]}" == "-t" ]] || [[ "${words[CURRENT-1]}" == "--to" ]]; then
                _pikpaktui_cloud_path
            fi
            ;;
        tasks)
            if (( CURRENT == 3 )); then
                local -a subcmds
                subcmds=(
                    'list:List offline tasks'
                    'ls:List offline tasks'
                    'retry:Retry a failed task'
                    'delete:Delete task(s)'
                    'rm:Delete task(s)'
                )
                _describe -t subcmds 'tasks subcommand' subcmds
            fi
            ;;
        star|unstar|info|cat|play)
            _pikpaktui_cloud_path
            ;;
        completions)
            if (( CURRENT == 3 )); then
                local -a shells
                shells=('zsh:Zsh completion script')
                _describe -t shells 'shell' shells
            fi
            ;;
    esac
}

_pikpaktui "$@"

# When sourced via eval, #compdef is just a comment — register explicitly.
if (( $+functions[compdef] )); then
    compdef _pikpaktui pikpaktui
    compdef _pikpaktui './target/release/pikpaktui' './target/debug/pikpaktui'
fi
"##;

pub fn run(args: &[String]) -> Result<()> {
    let shell = args.first().map(|s| s.as_str());
    match shell {
        Some("zsh") => {
            print!("{}", ZSH_COMPLETION);
            Ok(())
        }
        Some(other) => Err(anyhow!(
            "unsupported shell: {other}\nCurrently supported: zsh"
        )),
        None => Err(anyhow!(
            "Usage: pikpaktui completions <shell>\nCurrently supported: zsh"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_output_starts_with_compdef() {
        assert!(ZSH_COMPLETION.starts_with("#compdef pikpaktui"));
    }

    #[test]
    fn zsh_output_contains_main_function() {
        assert!(ZSH_COMPLETION.contains("_pikpaktui()"));
    }

    #[test]
    fn zsh_output_contains_cloud_path_helper() {
        assert!(ZSH_COMPLETION.contains("_pikpaktui_cloud_path()"));
    }

    #[test]
    fn zsh_output_contains_all_commands() {
        let commands = [
            "ls:", "mv:", "cp:", "rename:", "rm:", "mkdir:",
            "download:", "upload:", "share:", "offline:", "tasks:",
            "star:", "unstar:", "starred:", "events:",
            "trash:", "untrash:", "info:", "cat:", "play:",
            "quota:", "vip:", "completions:", "help:", "version:",
        ];
        for cmd in commands {
            assert!(
                ZSH_COMPLETION.contains(cmd),
                "Missing command in completion: {cmd}"
            );
        }
    }

    #[test]
    fn zsh_output_contains_compadd_prefix() {
        // Verify we use compadd -p for proper prefix-based matching
        assert!(ZSH_COMPLETION.contains("compadd -p"));
    }

    #[test]
    fn zsh_output_contains_explicit_compdef() {
        // For eval mode, must have explicit compdef
        assert!(ZSH_COMPLETION.contains("compdef _pikpaktui pikpaktui"));
    }

    #[test]
    fn run_zsh_succeeds() {
        let args = vec!["zsh".to_string()];
        assert!(run(&args).is_ok());
    }

    #[test]
    fn run_unknown_shell_errors() {
        let args = vec!["fish".to_string()];
        assert!(run(&args).is_err());
    }

    #[test]
    fn run_no_args_errors() {
        let args: Vec<String> = vec![];
        assert!(run(&args).is_err());
    }
}
