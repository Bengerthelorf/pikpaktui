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
                shells=(
                    'bash:Bash completion script'
                    'zsh:Zsh completion script'
                    'fish:Fish completion script'
                    'powershell:PowerShell completion script'
                )
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

const BASH_COMPLETION: &str = r##"# Bash completion for pikpaktui - PikPak cloud storage CLI/TUI
# Install: eval "$(pikpaktui completions bash)"
# Or:      pikpaktui completions bash > /etc/bash_completion.d/pikpaktui

_pikpaktui_cloud_path() {
    local bin="${COMP_WORDS[0]}"
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local dir partial

    if [[ -z "$cur" ]] || [[ "$cur" == "/" ]]; then
        dir="/"
        partial=""
    elif [[ "$cur" == */ ]]; then
        dir="$cur"
        partial=""
    elif [[ "$cur" == */* ]]; then
        dir="${cur%/*}/"
        partial="${cur##*/}"
    else
        dir="/"
        partial="$cur"
    fi
    [[ "$dir" == "//" ]] && dir="/"

    local IFS=$'\n'
    local -a entries
    mapfile -t entries < <("$bin" __complete_path "$dir" 2>/dev/null)
    for entry in "${entries[@]}"; do
        [[ -z "$entry" ]] && continue
        local full_path
        if [[ "$dir" == "/" ]]; then
            full_path="/${entry}"
        else
            full_path="${dir}${entry}"
        fi
        COMPREPLY+=("$full_path")
    done
    compopt -o nospace 2>/dev/null
}

_pikpaktui() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    local cmd="${COMP_WORDS[1]}"
    COMPREPLY=()

    local commands="ls mv cp rename rm mkdir download upload share offline tasks \
star unstar starred events trash untrash info link cat play quota vip login \
update completions help version"

    if [[ ${COMP_CWORD} -eq 1 ]]; then
        COMPREPLY=($(compgen -W "$commands" -- "$cur"))
        return
    fi

    case "$cmd" in
        ls)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-l --long -J --json -s --sort -r --reverse --tree --depth" -- "$cur"))
            elif [[ "$prev" == "-s" ]] || [[ "$prev" == "--sort" ]]; then
                COMPREPLY=($(compgen -W "name size created type extension none" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        mv|cp)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-t -n --dry-run" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        rename)
            _pikpaktui_cloud_path
            ;;
        rm)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-r --recursive -f --force -rf -fr" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        mkdir)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-p -n --dry-run" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        download)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-o --output -t -j --jobs -n --dry-run" -- "$cur"))
            elif [[ "$prev" == "-o" ]] || [[ "$prev" == "--output" ]]; then
                COMPREPLY=($(compgen -f -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        upload)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-t -n --dry-run" -- "$cur"))
            elif [[ "$prev" == "-t" ]]; then
                _pikpaktui_cloud_path
            else
                COMPREPLY=($(compgen -f -- "$cur"))
            fi
            ;;
        share)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-p --password -d --days -o -l -S -D -J --json -n --dry-run" -- "$cur"))
            elif [[ "$prev" == "-t" ]]; then
                _pikpaktui_cloud_path
            else
                _pikpaktui_cloud_path
            fi
            ;;
        offline)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-t --to -n --dry-run" -- "$cur"))
            elif [[ "$prev" == "-t" ]] || [[ "$prev" == "--to" ]]; then
                _pikpaktui_cloud_path
            fi
            ;;
        tasks)
            if [[ ${COMP_CWORD} -eq 2 ]]; then
                COMPREPLY=($(compgen -W "list ls retry delete rm" -- "$cur"))
            fi
            ;;
        star|unstar|info|link|cat|play|trash)
            _pikpaktui_cloud_path
            ;;
        completions)
            if [[ ${COMP_CWORD} -eq 2 ]]; then
                COMPREPLY=($(compgen -W "bash zsh fish powershell" -- "$cur"))
            fi
            ;;
    esac
}

complete -F _pikpaktui pikpaktui
complete -F _pikpaktui './target/release/pikpaktui'
complete -F _pikpaktui './target/debug/pikpaktui'
"##;

const FISH_COMPLETION: &str = r##"# Fish completion for pikpaktui - PikPak cloud storage CLI/TUI
# Install: pikpaktui completions fish | source
# Or:      pikpaktui completions fish > ~/.config/fish/completions/pikpaktui.fish

function __pikpaktui_cloud_path
    set -l cur (commandline -t)
    set -l bin (commandline -opc)[1]

    set -l dir "/"
    set -l partial $cur

    if string match -qr '^(.*/)([^/]*)$' -- $cur
        set dir $MATCH[2]
        set partial $MATCH[3]
    end

    set -l entries ($bin __complete_path $dir 2>/dev/null)
    for entry in $entries
        if string match -q "*/" -- $entry
            echo $dir(string replace -r '/$' '' -- $entry)/
        else
            echo $dir$entry
        end
    end
end

function __pikpaktui_using_command
    set -l cmd (commandline -opc)
    test (count $cmd) -ge 2 -a "$cmd[2]" = "$argv[1]"
end

# Disable default file completion for pikpaktui
complete -c pikpaktui -f

# Top-level commands
set -l subcommands ls mv cp rename rm mkdir download upload share offline tasks \
    star unstar starred events trash untrash info link cat play quota vip login \
    update completions help version

complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a ls         -d "List files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a mv         -d "Move files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a cp         -d "Copy files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a rename     -d "Rename file"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a rm         -d "Remove to trash"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a mkdir      -d "Create folder"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a download   -d "Download files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a upload     -d "Upload files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a share      -d "Share files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a offline    -d "Cloud download"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a tasks      -d "Manage tasks"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a star       -d "Star files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a unstar     -d "Unstar files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a starred    -d "List starred"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a events     -d "Recent events"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a trash      -d "Trashed files"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a untrash    -d "Restore from trash"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a info       -d "File info"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a link       -d "Direct download URL"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a cat        -d "Preview text file"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a play       -d "Play video"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a quota      -d "Storage quota"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a vip        -d "VIP info"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a login      -d "Login"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a update     -d "Update binary"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a completions -d "Generate completions"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a help       -d "Show help"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a version    -d "Show version"

# completions: shell name
complete -c pikpaktui -n "__pikpaktui_using_command completions" -a "bash zsh fish powershell"

# ls options
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s l -l long    -d "Long format"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s J -l json    -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s s -l sort    -d "Sort by field" -a "name size created type extension none"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s r -l reverse -d "Reverse sort"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -l tree         -d "Tree view"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -l depth        -d "Max depth"

# tasks subcommands
complete -c pikpaktui -n "__pikpaktui_using_command tasks" -a "list ls retry delete rm"
"##;

const POWERSHELL_COMPLETION: &str = r##"# PowerShell completion for pikpaktui - PikPak cloud storage CLI/TUI
# Install: pikpaktui completions powershell | Out-String | Invoke-Expression
# Or:      pikpaktui completions powershell > $PROFILE.CurrentUserCurrentHost.Replace("profile.ps1","pikpaktui.ps1")
#          then add: . "$PROFILE.CurrentUserCurrentHost.Replace("profile.ps1","pikpaktui.ps1")"

Register-ArgumentCompleter -Native -CommandName @('pikpaktui') -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $elements = $commandAst.CommandElements
    $command  = if ($elements.Count -gt 1) { $elements[1].ToString() } else { "" }

    function Get-CloudPaths {
        param([string]$prefix)
        $dir     = "/"
        $partial = $prefix
        if ($prefix -match '^(.*/)([^/]*)$') {
            $dir     = if ($Matches[1] -eq "/") { "/" } else { $Matches[1] }
            $partial = $Matches[2]
        } elseif ($prefix -eq "" -or $prefix -eq "/") {
            $dir     = "/"
            $partial = ""
        }
        try {
            $entries = & pikpaktui __complete_path $dir 2>$null
            foreach ($entry in $entries) {
                $fullPath = if ($dir -eq "/") { "/$entry" } else { "$dir$entry" }
                if ($partial -eq "" -or $fullPath -like "*$partial*") {
                    [System.Management.Automation.CompletionResult]::new(
                        $fullPath, $fullPath, 'ParameterValue', $fullPath)
                }
            }
        } catch {}
    }

    $allCommands = @(
        'ls','mv','cp','rename','rm','mkdir','download','upload','share',
        'offline','tasks','star','unstar','starred','events','trash','untrash',
        'info','link','cat','play','quota','vip','login','update','completions',
        'help','version'
    )

    # Top-level: no sub-command typed yet (or user is still completing the command name)
    if ($elements.Count -le 1 -or
        ($elements.Count -eq 2 -and $wordToComplete -ne "" -and $command -eq $wordToComplete)) {
        return $allCommands | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
        }
    }

    switch ($command) {
        "completions" {
            @('bash','zsh','fish','powershell') |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
        }
        "tasks" {
            @('list','ls','retry','delete','rm') |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
        }
        { $_ -in @('ls','mv','cp','rename','rm','mkdir','download','upload',
                    'share','offline','star','unstar','info','link','cat','play','trash') } {
            if ($wordToComplete.StartsWith('-')) {
                $opts = switch ($command) {
                    'ls'       { @('-l','--long','-J','--json','-s','--sort','-r','--reverse','--tree','--depth') }
                    'mv'       { @('-t','-n','--dry-run') }
                    'cp'       { @('-t','-n','--dry-run') }
                    'rename'   { @('-n','--dry-run') }
                    'rm'       { @('-r','--recursive','-f','--force','-rf','-fr') }
                    'mkdir'    { @('-p','-n','--dry-run') }
                    'download' { @('-o','--output','-t','-j','--jobs','-n','--dry-run') }
                    'upload'   { @('-t','-n','--dry-run') }
                    'share'    { @('-p','--password','-d','--days','-o','-l','-S','-D','-J','--json','-n','--dry-run') }
                    'offline'  { @('-t','--to','-n','--dry-run') }
                    default    { @() }
                }
                $opts | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
            } else {
                Get-CloudPaths $wordToComplete
            }
        }
        default { Get-CloudPaths $wordToComplete }
    }
}
"##;

pub fn run(args: &[String]) -> Result<()> {
    let shell = args.first().map(|s| s.as_str());
    match shell {
        Some("zsh") => {
            print!("{}", ZSH_COMPLETION);
            Ok(())
        }
        Some("bash") => {
            print!("{}", BASH_COMPLETION);
            Ok(())
        }
        Some("fish") => {
            print!("{}", FISH_COMPLETION);
            Ok(())
        }
        Some("powershell") | Some("pwsh") => {
            print!("{}", POWERSHELL_COMPLETION);
            Ok(())
        }
        Some(other) => Err(anyhow!(
            "unsupported shell: {other}\nSupported: bash, zsh, fish, powershell"
        )),
        None => Err(anyhow!(
            "Usage: pikpaktui completions <shell>\nSupported: bash, zsh, fish, powershell"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Zsh ──────────────────────────────────────────────────────────────────

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
            assert!(ZSH_COMPLETION.contains(cmd), "Missing command in zsh completion: {cmd}");
        }
    }

    #[test]
    fn zsh_output_contains_compadd_prefix() {
        assert!(ZSH_COMPLETION.contains("compadd -p"));
    }

    #[test]
    fn zsh_output_contains_explicit_compdef() {
        assert!(ZSH_COMPLETION.contains("compdef _pikpaktui pikpaktui"));
    }

    #[test]
    fn zsh_lists_all_four_shells() {
        assert!(ZSH_COMPLETION.contains("'bash:Bash completion script'"));
        assert!(ZSH_COMPLETION.contains("'zsh:Zsh completion script'"));
        assert!(ZSH_COMPLETION.contains("'fish:Fish completion script'"));
        assert!(ZSH_COMPLETION.contains("'powershell:PowerShell completion script'"));
    }

    // ── Bash ─────────────────────────────────────────────────────────────────

    #[test]
    fn bash_output_contains_complete_directive() {
        assert!(BASH_COMPLETION.contains("complete -F _pikpaktui pikpaktui"));
    }

    #[test]
    fn bash_output_contains_cloud_path_helper() {
        assert!(BASH_COMPLETION.contains("_pikpaktui_cloud_path()"));
    }

    #[test]
    fn bash_output_contains_all_commands() {
        let commands = [
            "ls", "mv", "cp", "rename", "rm", "mkdir",
            "download", "upload", "share", "offline", "tasks",
            "star", "unstar", "starred", "events",
            "trash", "untrash", "info", "cat", "play",
            "quota", "vip", "completions", "help", "version",
        ];
        for cmd in commands {
            assert!(BASH_COMPLETION.contains(cmd), "Missing command in bash completion: {cmd}");
        }
    }

    #[test]
    fn bash_output_lists_all_four_shells() {
        assert!(BASH_COMPLETION.contains("bash zsh fish powershell"));
    }

    // ── Fish ─────────────────────────────────────────────────────────────────

    #[test]
    fn fish_output_contains_complete_directives() {
        assert!(FISH_COMPLETION.contains("complete -c pikpaktui"));
    }

    #[test]
    fn fish_output_contains_all_commands() {
        let commands = [
            "ls", "mv", "cp", "rename", "rm", "mkdir",
            "download", "upload", "share", "offline", "tasks",
            "star", "unstar", "starred", "events",
            "trash", "untrash", "info", "cat", "play",
            "quota", "vip", "completions", "help", "version",
        ];
        for cmd in commands {
            assert!(FISH_COMPLETION.contains(cmd), "Missing command in fish completion: {cmd}");
        }
    }

    #[test]
    fn fish_output_lists_all_four_shells() {
        assert!(FISH_COMPLETION.contains("bash zsh fish powershell"));
    }

    // ── PowerShell ────────────────────────────────────────────────────────────

    #[test]
    fn powershell_output_contains_register_argument_completer() {
        assert!(POWERSHELL_COMPLETION.contains("Register-ArgumentCompleter"));
    }

    #[test]
    fn powershell_output_contains_native_flag() {
        assert!(POWERSHELL_COMPLETION.contains("-Native"));
    }

    #[test]
    fn powershell_output_contains_cloud_path_helper() {
        assert!(POWERSHELL_COMPLETION.contains("Get-CloudPaths"));
    }

    #[test]
    fn powershell_output_contains_all_commands() {
        let commands = [
            "'ls'", "'mv'", "'cp'", "'rename'", "'rm'", "'mkdir'",
            "'download'", "'upload'", "'share'", "'offline'", "'tasks'",
            "'star'", "'unstar'", "'starred'", "'events'",
            "'trash'", "'untrash'", "'info'", "'cat'", "'play'",
            "'quota'", "'vip'", "'completions'", "'help'", "'version'",
        ];
        for cmd in commands {
            assert!(POWERSHELL_COMPLETION.contains(cmd), "Missing command in powershell completion: {cmd}");
        }
    }

    #[test]
    fn powershell_output_lists_all_four_shells() {
        assert!(POWERSHELL_COMPLETION.contains("'bash','zsh','fish','powershell'"));
    }

    // ── run() dispatch ────────────────────────────────────────────────────────

    #[test]
    fn run_zsh_succeeds() {
        assert!(run(&["zsh".to_string()]).is_ok());
    }

    #[test]
    fn run_bash_succeeds() {
        assert!(run(&["bash".to_string()]).is_ok());
    }

    #[test]
    fn run_fish_succeeds() {
        assert!(run(&["fish".to_string()]).is_ok());
    }

    #[test]
    fn run_powershell_succeeds() {
        assert!(run(&["powershell".to_string()]).is_ok());
    }

    #[test]
    fn run_pwsh_alias_succeeds() {
        assert!(run(&["pwsh".to_string()]).is_ok());
    }

    #[test]
    fn run_unknown_shell_errors() {
        assert!(run(&["nushell".to_string()]).is_err());
    }

    #[test]
    fn run_no_args_errors() {
        let args: Vec<String> = vec![];
        assert!(run(&args).is_err());
    }
}
