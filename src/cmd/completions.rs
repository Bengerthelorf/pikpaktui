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
        'empty:Permanently delete items from trash'
        'info:Show detailed file/folder info'
        'link:Get direct download URL'
        'cat:Preview text file contents'
        'play:Play video with external player'
        'quota:Show storage quota'
        'vip:Show VIP & account info'
        'whoami:Show logged-in account identity'
        'login:Log in and save credentials'
        'update:Check for updates and self-update'
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
                compadd -- '-l' '--long' '-J' '--json' '-s' '--sort' '-r' '--reverse' '--tree' '--depth'
            elif [[ "${words[CURRENT-1]}" == "-s" ]] || [[ "${words[CURRENT-1]}" == "--sort" ]]; then
                compadd -- 'name' 'size' 'created' 'type' 'extension' 'none'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        mv|cp)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-t' '-n' '--dry-run'
            elif [[ "${words[CURRENT-1]}" == "-t" ]]; then
                _pikpaktui_cloud_path
            else
                _pikpaktui_cloud_path
            fi
            ;;
        rename)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-n' '--dry-run'
            elif (( CURRENT == 3 )); then
                _pikpaktui_cloud_path
            fi
            ;;
        rm)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-r' '--recursive' '-f' '--force' '-rf' '-fr' '-n' '--dry-run'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        mkdir)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-p' '-n' '--dry-run'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        download)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-o' '--output' '-t' '-j' '--jobs' '-n' '--dry-run'
            elif [[ "${words[CURRENT-1]}" == "-o" ]] || [[ "${words[CURRENT-1]}" == "--output" ]] || [[ "${words[CURRENT-1]}" == "-t" ]]; then
                _files
            else
                _pikpaktui_cloud_path
            fi
            ;;
        upload)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-t' '-n' '--dry-run'
            elif [[ "${words[CURRENT-1]}" == "-t" ]]; then
                _pikpaktui_cloud_path
            else
                _files
            fi
            ;;
        share)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-p' '--password' '--pass-code' '-d' '--days' '-o' '-l' '--list' '-S' '--save' '-b' '--browse' '-D' '--delete' '-t' '--to' '-J' '--json' '-n' '--dry-run'
            elif [[ "${words[CURRENT-1]}" == "-o" ]]; then
                _files
            elif [[ "${words[CURRENT-1]}" == "-t" ]] || [[ "${words[CURRENT-1]}" == "--to" ]]; then
                _pikpaktui_cloud_path
            else
                _pikpaktui_cloud_path
            fi
            ;;
        offline)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-t' '--to' '--name' '-p' '--preview' '-n' '--dry-run'
            elif [[ "${words[CURRENT-1]}" == "-t" ]] || [[ "${words[CURRENT-1]}" == "--to" ]]; then
                _pikpaktui_cloud_path
            fi
            ;;
        tasks)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-J' '--json' '-n' '--dry-run'
            elif (( CURRENT == 3 )); then
                local -a subcmds
                subcmds=(
                    'list:List offline tasks'
                    'ls:List offline tasks'
                    'show:Show one task'
                    'retry:Retry a failed task'
                    'delete:Delete task(s)'
                    'rm:Delete task(s)'
                )
                _describe -t subcmds 'tasks subcommand' subcmds
            fi
            ;;
        empty)
            [[ "${words[CURRENT]}" == -* ]] && compadd -- '--all' '-r' '--recursive' '-f' '--force' '-n' '--dry-run'
            ;;
        star|unstar)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-n' '--dry-run'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        info)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-J' '--json'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        link)
            if [[ "${words[CURRENT]}" == -* ]]; then
                compadd -- '-m' '--media' '-c' '--copy' '-J' '--json'
            else
                _pikpaktui_cloud_path
            fi
            ;;
        cat|play)
            _pikpaktui_cloud_path
            ;;
        starred|trash)
            [[ "${words[CURRENT]}" == -* ]] && compadd -- '-l' '--long' '-J' '--json'
            ;;
        events)
            [[ "${words[CURRENT]}" == -* ]] && compadd -- '-J' '--json'
            ;;
        untrash)
            [[ "${words[CURRENT]}" == -* ]] && compadd -- '-n' '--dry-run'
            ;;
        login)
            [[ "${words[CURRENT]}" == -* ]] && compadd -- '-u' '--user' '-p' '--password'
            ;;
        whoami|quota)
            [[ "${words[CURRENT]}" == -* ]] && compadd -- '-J' '--json'
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

    local entry
    while IFS= read -r entry; do
        [[ -z "$entry" ]] && continue
        [[ -n "$partial" && "$entry" != "$partial"* ]] && continue
        local full_path
        if [[ "$dir" == "/" ]]; then
            full_path="/${entry}"
        else
            full_path="${dir}${entry}"
        fi
        COMPREPLY+=("$full_path")
    done < <("$bin" __complete_path "$dir" 2>/dev/null)
    if type compopt >/dev/null 2>&1; then
        compopt -o nospace
    fi
}

_pikpaktui_local_path() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local entry
    while IFS= read -r entry; do
        [[ -z "$entry" ]] && continue
        COMPREPLY+=("$entry")
    done < <(compgen -f -- "$cur")
}

_pikpaktui() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local prev="${COMP_WORDS[COMP_CWORD-1]}"
    local cmd="${COMP_WORDS[1]}"
    COMPREPLY=()

    local commands="ls mv cp rename rm mkdir download upload share offline tasks \
star unstar starred events trash untrash empty info link cat play quota vip \
whoami login update completions help version"

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
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-n --dry-run" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        rm)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-r --recursive -f --force -rf -fr -n --dry-run" -- "$cur"))
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
            elif [[ "$prev" == "-o" ]] || [[ "$prev" == "--output" ]] || [[ "$prev" == "-t" ]]; then
                _pikpaktui_local_path
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
                _pikpaktui_local_path
            fi
            ;;
        share)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-p --password --pass-code -d --days -o -l --list -S --save -b --browse -D --delete -t --to -J --json -n --dry-run" -- "$cur"))
            elif [[ "$prev" == "-o" ]]; then
                _pikpaktui_local_path
            elif [[ "$prev" == "-t" ]] || [[ "$prev" == "--to" ]]; then
                _pikpaktui_cloud_path
            else
                _pikpaktui_cloud_path
            fi
            ;;
        offline)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-t --to --name -p --preview -n --dry-run" -- "$cur"))
            elif [[ "$prev" == "-t" ]] || [[ "$prev" == "--to" ]]; then
                _pikpaktui_cloud_path
            fi
            ;;
        tasks)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-J --json -n --dry-run" -- "$cur"))
            elif [[ ${COMP_CWORD} -eq 2 ]]; then
                COMPREPLY=($(compgen -W "list ls show retry delete rm" -- "$cur"))
            fi
            ;;
        star|unstar)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-n --dry-run" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        info)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-J --json" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        link)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-m --media -c --copy -J --json" -- "$cur"))
            else
                _pikpaktui_cloud_path
            fi
            ;;
        cat|play)
            _pikpaktui_cloud_path
            ;;
        starred|trash)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-l --long -J --json" -- "$cur"))
            fi
            ;;
        events)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-J --json" -- "$cur"))
            fi
            ;;
        untrash)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-n --dry-run" -- "$cur"))
            fi
            ;;
        empty)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "--all -r --recursive -f --force -n --dry-run" -- "$cur"))
            fi
            ;;
        login)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-u --user -p --password" -- "$cur"))
            fi
            ;;
        whoami|quota)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=($(compgen -W "-J --json" -- "$cur"))
            fi
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
    if string match -q "*/*" -- $cur
        set dir (string replace -r '[^/]*$' '' -- $cur)
        test -n "$dir"; or set dir "/"
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

function __pikpaktui_prev_is
    set -l cmd (commandline -opc)
    test (count $cmd) -ge 2; and contains -- $cmd[-1] $argv
end

# Disable default file completion for pikpaktui
complete -c pikpaktui -f

# Top-level commands
set -l subcommands ls mv cp rename rm mkdir download upload share offline tasks \
    star unstar starred events trash untrash empty info link cat play quota vip \
    whoami login update completions help version

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
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a empty      -d "Permanently delete from trash"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a info       -d "File info"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a link       -d "Direct download URL"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a cat        -d "Preview text file"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a play       -d "Play video"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a quota      -d "Storage quota"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a vip        -d "VIP info"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a whoami     -d "Account identity"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a login      -d "Login"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a update     -d "Update binary"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a completions -d "Generate completions"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a help       -d "Show help"
complete -c pikpaktui -n "not __fish_seen_subcommand_from $subcommands" -a version    -d "Show version"

# completions: shell name
complete -c pikpaktui -n "__pikpaktui_using_command completions" -a "bash zsh fish powershell"

# Context-aware path candidates
complete -c pikpaktui -n "__pikpaktui_using_command ls; and not __pikpaktui_prev_is -s --sort --depth" -a "(__pikpaktui_cloud_path)"
complete -c pikpaktui -n "__pikpaktui_using_command mv; or __pikpaktui_using_command cp; or __pikpaktui_using_command rename; or __pikpaktui_using_command rm; or __pikpaktui_using_command mkdir; or __pikpaktui_using_command star; or __pikpaktui_using_command unstar; or __pikpaktui_using_command info; or __pikpaktui_using_command link; or __pikpaktui_using_command cat; or __pikpaktui_using_command play" -a "(__pikpaktui_cloud_path)"
complete -c pikpaktui -n "__pikpaktui_using_command download; and not __pikpaktui_prev_is -o --output -t -j --jobs" -a "(__pikpaktui_cloud_path)"
complete -c pikpaktui -n "__pikpaktui_using_command download; and __pikpaktui_prev_is -o --output -t" -F
complete -c pikpaktui -n "__pikpaktui_using_command upload; and __pikpaktui_prev_is -t" -a "(__pikpaktui_cloud_path)"
complete -c pikpaktui -n "__pikpaktui_using_command upload; and not __pikpaktui_prev_is -t" -F
complete -c pikpaktui -n "__pikpaktui_using_command share; and __pikpaktui_prev_is -o" -F
complete -c pikpaktui -n "__pikpaktui_using_command share; and not __pikpaktui_prev_is -o -d --days --pass-code" -a "(__pikpaktui_cloud_path)"
complete -c pikpaktui -n "__pikpaktui_using_command offline; and __pikpaktui_prev_is -t --to" -a "(__pikpaktui_cloud_path)"

# ls options
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s l -l long    -d "Long format"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s J -l json    -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s s -l sort    -d "Sort by field" -a "name size created type extension none"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -s r -l reverse -d "Reverse sort"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -l tree         -d "Tree view"
complete -c pikpaktui -n "__pikpaktui_using_command ls" -l depth        -d "Max depth"

# File operation and transfer options
complete -c pikpaktui -n "__pikpaktui_using_command mv; or __pikpaktui_using_command cp" -s t -d "Batch destination"
complete -c pikpaktui -n "__pikpaktui_using_command mv; or __pikpaktui_using_command cp; or __pikpaktui_using_command rename; or __pikpaktui_using_command mkdir; or __pikpaktui_using_command download; or __pikpaktui_using_command upload" -s n -l dry-run -d "Preview without executing"
complete -c pikpaktui -n "__pikpaktui_using_command rm" -s r -l recursive -d "Remove folders recursively"
complete -c pikpaktui -n "__pikpaktui_using_command rm" -s f -l force -d "Permanently delete"
complete -c pikpaktui -n "__pikpaktui_using_command rm" -s n -l dry-run -d "Preview without executing"
complete -c pikpaktui -n "__pikpaktui_using_command mkdir" -s p -d "Create intermediate directories"
complete -c pikpaktui -n "__pikpaktui_using_command download" -s o -l output -d "Output file"
complete -c pikpaktui -n "__pikpaktui_using_command download; or __pikpaktui_using_command upload" -s t -d "Batch destination"
complete -c pikpaktui -n "__pikpaktui_using_command download" -s j -l jobs -d "Concurrent downloads"

# Share and cloud-download options
complete -c pikpaktui -n "__pikpaktui_using_command share" -s p -l password -d "Password-protect a new share"
complete -c pikpaktui -n "__pikpaktui_using_command share" -l pass-code -d "Share pass code"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s d -l days -d "Expiry in days"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s o -d "Write share URL to file"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s l -l list -d "List shares"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s S -l save -d "Save a share"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s b -l browse -d "Browse a share"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s D -l delete -d "Delete shares"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s t -l to -d "Destination folder"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s J -l json -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command share" -s n -l dry-run -d "Preview without saving"
complete -c pikpaktui -n "__pikpaktui_using_command offline" -s t -l to -d "Destination folder"
complete -c pikpaktui -n "__pikpaktui_using_command offline" -l name -d "Custom task name"
complete -c pikpaktui -n "__pikpaktui_using_command offline" -s p -l preview -d "Inspect without adding"
complete -c pikpaktui -n "__pikpaktui_using_command offline" -s n -l dry-run -d "Preview without creating"

# tasks subcommands
complete -c pikpaktui -n "__pikpaktui_using_command tasks" -a "list ls show retry delete rm"
complete -c pikpaktui -n "__pikpaktui_using_command tasks" -s J -l json -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command tasks" -s n -l dry-run -d "Preview without executing"

# Account, auth, and trash options
complete -c pikpaktui -n "__pikpaktui_using_command empty" -l all -d "Empty all trash"
complete -c pikpaktui -n "__pikpaktui_using_command empty" -s r -l recursive -d "Empty all trash"
complete -c pikpaktui -n "__pikpaktui_using_command empty" -s f -l force -d "Skip confirmation"
complete -c pikpaktui -n "__pikpaktui_using_command empty" -s n -l dry-run -d "Preview without deleting"
complete -c pikpaktui -n "__pikpaktui_using_command login" -s u -l user -d "Account email"
complete -c pikpaktui -n "__pikpaktui_using_command login" -s p -l password -d "Account password"
complete -c pikpaktui -n "__pikpaktui_using_command whoami; or __pikpaktui_using_command quota" -s J -l json -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command star; or __pikpaktui_using_command unstar" -s n -l dry-run -d "Preview without changing stars"
complete -c pikpaktui -n "__pikpaktui_using_command info" -s J -l json -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command link" -s m -l media -d "Show media stream URLs"
complete -c pikpaktui -n "__pikpaktui_using_command link" -s c -l copy -d "Copy URL to clipboard"
complete -c pikpaktui -n "__pikpaktui_using_command link" -s J -l json -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command starred; or __pikpaktui_using_command trash" -s l -l long -d "Long format"
complete -c pikpaktui -n "__pikpaktui_using_command starred; or __pikpaktui_using_command trash; or __pikpaktui_using_command events" -s J -l json -d "JSON output"
complete -c pikpaktui -n "__pikpaktui_using_command untrash" -s n -l dry-run -d "Preview without restoring"
"##;

const POWERSHELL_COMPLETION: &str = r##"# PowerShell completion for pikpaktui - PikPak cloud storage CLI/TUI
# Install: pikpaktui completions powershell | Out-String | Invoke-Expression
# Or:      pikpaktui completions powershell > $PROFILE.CurrentUserCurrentHost.Replace("profile.ps1","pikpaktui.ps1")
#          then add: . "$PROFILE.CurrentUserCurrentHost.Replace("profile.ps1","pikpaktui.ps1")"

Register-ArgumentCompleter -Native -CommandName @('pikpaktui') -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $elements = $commandAst.CommandElements
    $command  = if ($elements.Count -gt 1) { $elements[1].ToString() } else { "" }
    $binary   = if ($elements.Count -gt 0) { $elements[0].ToString() } else { "pikpaktui" }
    $currentIsElement = $wordToComplete -ne "" -and
        $elements.Count -gt 0 -and $elements[-1].ToString() -eq $wordToComplete
    $previousIndex = if ($currentIsElement) { $elements.Count - 2 } else { $elements.Count - 1 }
    $previous = if ($previousIndex -ge 0) { $elements[$previousIndex].ToString() } else { "" }

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
            $entries = & $binary __complete_path $dir 2>$null
            foreach ($entry in $entries) {
                $fullPath = if ($dir -eq "/") { "/$entry" } else { "$dir$entry" }
                if ($partial -eq "" -or
                    $entry.StartsWith($partial, [System.StringComparison]::OrdinalIgnoreCase)) {
                    [System.Management.Automation.CompletionResult]::new(
                        $fullPath, $fullPath, 'ParameterValue', $fullPath)
                }
            }
        } catch {}
    }

    function Get-LocalPaths {
        param([string]$prefix)
        $slash = [Math]::Max($prefix.LastIndexOf('/'), $prefix.LastIndexOf('\'))
        if ($slash -ge 0) {
            $displayDir = $prefix.Substring(0, $slash + 1)
            $leaf = $prefix.Substring($slash + 1)
            $searchDir = $displayDir
        } else {
            $displayDir = ""
            $leaf = $prefix
            $searchDir = "."
        }
        try {
            Get-ChildItem -LiteralPath $searchDir -Force -ErrorAction Stop |
                Where-Object {
                    $_.Name.StartsWith($leaf, [System.StringComparison]::OrdinalIgnoreCase)
                } |
                ForEach-Object {
                    $candidate = "$displayDir$($_.Name)"
                    if ($_.PSIsContainer) {
                        $candidate += [System.IO.Path]::DirectorySeparatorChar
                    }
                    [System.Management.Automation.CompletionResult]::new(
                        $candidate, $candidate, 'ParameterValue', $candidate)
                }
        } catch {}
    }

    $allCommands = @(
        'ls','mv','cp','rename','rm','mkdir','download','upload','share',
        'offline','tasks','star','unstar','starred','events','trash','untrash',
        'empty','info','link','cat','play','quota','vip','whoami','login',
        'update','completions','help','version'
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
            $candidates = if ($wordToComplete.StartsWith('-')) {
                @('-J','--json','-n','--dry-run')
            } else {
                @('list','ls','show','retry','delete','rm')
            }
            $candidates |
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
                    'rm'       { @('-r','--recursive','-f','--force','-rf','-fr','-n','--dry-run') }
                    'mkdir'    { @('-p','-n','--dry-run') }
                    'download' { @('-o','--output','-t','-j','--jobs','-n','--dry-run') }
                    'upload'   { @('-t','-n','--dry-run') }
                    'share'    { @('-p','--password','--pass-code','-d','--days','-o','-l','--list','-S','--save','-b','--browse','-D','--delete','-t','--to','-J','--json','-n','--dry-run') }
                    'offline'  { @('-t','--to','--name','-p','--preview','-n','--dry-run') }
                    'star'     { @('-n','--dry-run') }
                    'unstar'   { @('-n','--dry-run') }
                    'info'     { @('-J','--json') }
                    'link'     { @('-m','--media','-c','--copy','-J','--json') }
                    'trash'    { @('-l','--long','-J','--json') }
                    default    { @() }
                }
                $opts | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
            } elseif ($command -eq 'download' -and $previous -in @('-o','--output','-t')) {
                Get-LocalPaths $wordToComplete
            } elseif ($command -eq 'upload' -and $previous -ne '-t') {
                Get-LocalPaths $wordToComplete
            } elseif ($command -eq 'share' -and $previous -eq '-o') {
                Get-LocalPaths $wordToComplete
            } elseif ($command -eq 'offline' -and $previous -notin @('-t','--to')) {
                return
            } elseif ($command -eq 'trash') {
                return
            } else {
                Get-CloudPaths $wordToComplete
            }
        }
        { $_ -in @('starred','events','untrash') } {
            $opts = switch ($command) {
                'starred' { @('-l','--long','-J','--json') }
                'events'  { @('-J','--json') }
                'untrash' { @('-n','--dry-run') }
            }
            $opts |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
        }
        "empty" {
            @('--all','-r','--recursive','-f','--force','-n','--dry-run') |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
        }
        "login" {
            @('-u','--user','-p','--password') |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
        }
        { $_ -in @('whoami','quota') } {
            @('-J','--json') |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
                }
        }
        default {}
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

    fn public_commands() -> impl Iterator<Item = &'static str> {
        crate::cmd::COMMAND_GROUPS
            .iter()
            .flat_map(|(_, commands)| commands.iter().copied())
            .chain(["help", "version"])
    }

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
        for command in public_commands() {
            let cmd = format!("'{command}:");
            assert!(
                ZSH_COMPLETION.contains(&cmd),
                "Missing command in zsh completion: {cmd}"
            );
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
        let command_list = BASH_COMPLETION
            .split("local commands=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("bash top-level command list");
        for cmd in public_commands() {
            assert!(
                command_list.split_whitespace().any(|entry| entry == cmd),
                "Missing command in bash completion: {cmd}"
            );
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
        for cmd in public_commands() {
            let directive = format!("-a {cmd} ");
            assert!(
                FISH_COMPLETION.contains(&directive),
                "Missing command in fish completion: {cmd}"
            );
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
        let command_list = POWERSHELL_COMPLETION
            .split("$allCommands = @(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("PowerShell top-level command list");
        for command in public_commands() {
            let cmd = format!("'{command}'");
            assert!(
                command_list.contains(&cmd),
                "Missing command in powershell completion: {cmd}"
            );
        }
    }

    #[test]
    fn powershell_output_lists_all_four_shells() {
        assert!(POWERSHELL_COMPLETION.contains("'bash','zsh','fish','powershell'"));
    }

    #[test]
    fn all_shells_complete_current_tasks_subcommands() {
        for (shell, script) in [
            ("zsh", ZSH_COMPLETION),
            ("bash", BASH_COMPLETION),
            ("fish", FISH_COMPLETION),
            ("powershell", POWERSHELL_COMPLETION),
        ] {
            assert!(
                script.contains("show"),
                "{shell} completion is missing `tasks show`"
            );
        }
    }

    #[test]
    fn all_shells_complete_current_offline_options() {
        for (shell, script) in [
            ("zsh", ZSH_COMPLETION),
            ("bash", BASH_COMPLETION),
            ("fish", FISH_COMPLETION),
            ("powershell", POWERSHELL_COMPLETION),
        ] {
            for option in ["--name", "--preview", "--dry-run"] {
                let needle = if shell == "fish" {
                    format!("-l {}", option.trim_start_matches('-'))
                } else {
                    option.to_string()
                };
                assert!(
                    script.contains(&needle),
                    "{shell} completion is missing `offline {option}`"
                );
            }
        }
    }

    #[test]
    fn all_shells_complete_current_share_modes() {
        for (shell, script) in [
            ("zsh", ZSH_COMPLETION),
            ("bash", BASH_COMPLETION),
            ("fish", FISH_COMPLETION),
            ("powershell", POWERSHELL_COMPLETION),
        ] {
            for option in ["--browse", "--save", "--list", "--delete"] {
                let needle = if shell == "fish" {
                    format!("-l {}", option.trim_start_matches('-'))
                } else {
                    option.to_string()
                };
                assert!(
                    script.contains(&needle),
                    "{shell} completion is missing `share {option}`"
                );
            }
        }
    }

    #[test]
    fn fish_wires_dynamic_cloud_path_candidates_into_completions() {
        assert!(
            FISH_COMPLETION.contains("-a \"(__pikpaktui_cloud_path)\""),
            "Fish defines the cloud-path helper but never uses its candidates"
        );
    }

    #[test]
    fn fish_reenables_local_file_candidates_only_for_local_path_contexts() {
        for command in ["download", "upload"] {
            let directive = format!("__pikpaktui_using_command {command}");
            assert!(
                FISH_COMPLETION
                    .lines()
                    .any(|line| line.contains(&directive) && line.contains(" -F")),
                "Fish does not enable local file completion for {command}"
            );
        }
    }

    #[test]
    fn bash_distinguishes_local_download_targets_from_cloud_sources() {
        assert!(
            BASH_COMPLETION.contains(
                r#""$prev" == "-o" ]] || [[ "$prev" == "--output" ]] || [[ "$prev" == "-t""#
            ),
            "Bash must complete download -o/--output/-t values as local paths"
        );
    }

    #[test]
    fn powershell_distinguishes_local_transfer_paths_from_cloud_paths() {
        assert!(POWERSHELL_COMPLETION.contains("function Get-LocalPaths"));
        assert!(
            POWERSHELL_COMPLETION.contains(r#"$previous -in @('-o','--output','-t')"#),
            "PowerShell must treat download output and -t values as local paths"
        );
        assert!(
            POWERSHELL_COMPLETION.contains(r#"$command -eq 'upload' -and $previous -ne '-t'"#),
            "PowerShell must treat upload source paths as local paths"
        );
    }

    #[test]
    fn every_shell_exposes_flags_supported_by_existing_parsers() {
        fn case_arm<'a>(script: &'a str, marker: &str) -> &'a str {
            let marker = format!("\n        {marker})");
            script
                .split_once(&marker)
                .unwrap_or_else(|| panic!("missing shell case arm {marker}"))
                .1
                .split_once("\n            ;;")
                .expect("unterminated shell case arm")
                .0
        }

        let shell_case_contracts: &[(&str, &[&str])] = &[
            ("star|unstar", &["--dry-run"]),
            ("info", &["--json"]),
            ("link", &["--media", "--copy", "--json"]),
            ("starred|trash", &["--long", "--json"]),
            ("events", &["--json"]),
            ("untrash", &["--dry-run"]),
        ];
        for (shell, script) in [("zsh", ZSH_COMPLETION), ("bash", BASH_COMPLETION)] {
            for (marker, options) in shell_case_contracts {
                let arm = case_arm(script, marker);
                for option in *options {
                    assert!(
                        arm.contains(option),
                        "{shell} completion case `{marker}` is missing `{option}`"
                    );
                }
            }
        }

        let line_contracts: &[(&str, &[&str])] = &[
            ("star", &["--dry-run"]),
            ("unstar", &["--dry-run"]),
            ("info", &["--json"]),
            ("link", &["--media", "--copy", "--json"]),
            ("starred", &["--long", "--json"]),
            ("events", &["--json"]),
            ("trash", &["--long", "--json"]),
            ("untrash", &["--dry-run"]),
        ];
        for (shell, script) in [
            ("fish", FISH_COMPLETION),
            ("powershell", POWERSHELL_COMPLETION),
        ] {
            for (command, options) in line_contracts {
                for option in *options {
                    let option_needle = if shell == "fish" {
                        format!("-l {}", option.trim_start_matches('-'))
                    } else {
                        option.to_string()
                    };
                    let command_needle = if shell == "fish" {
                        format!("__pikpaktui_using_command {command}")
                    } else {
                        format!("'{command}'")
                    };
                    assert!(
                        script.lines().any(|line| {
                            line.contains(&command_needle) && line.contains(&option_needle)
                        }),
                        "{shell} completion is missing `{command} {option}`"
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn bash_cloud_completion_works_without_the_mapfile_builtin() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::process::Command;

        let fixture = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("completion-bash32-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fixture);
        std::fs::create_dir_all(&fixture).unwrap();

        let completion_path = fixture.join("pikpaktui-completion.bash");
        std::fs::write(&completion_path, BASH_COMPLETION).unwrap();
        let mock_path = fixture.join("pikpaktui-mock");
        std::fs::write(
            &mock_path,
            "#!/bin/bash\nif [[ \"$1\" == \"__complete_path\" ]]; then\n  printf 'Alpha/\\nbeta.txt\\n'\nfi\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&mock_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&mock_path, permissions).unwrap();

        let output = Command::new("bash")
            .args([
                "-c",
                r#"
enable -n mapfile 2>/dev/null || true
source "$1"
COMP_WORDS=("$2" "ls" "/A")
COMP_CWORD=2
_pikpaktui
printf '%s\n' "${COMPREPLY[@]}"
"#,
                "completion-test",
            ])
            .arg(&completion_path)
            .arg(&mock_path)
            .output()
            .unwrap();

        let _ = std::fs::remove_dir_all(&fixture);
        assert!(
            output.status.success(),
            "bash completion failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "/Alpha/\n");
    }

    #[cfg(unix)]
    #[test]
    fn bash_local_completion_preserves_spaces_in_file_names() {
        use std::process::Command;

        let fixture = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("completion-bash-spaces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fixture);
        std::fs::create_dir_all(&fixture).unwrap();

        let completion_path = fixture.join("pikpaktui-completion.bash");
        std::fs::write(&completion_path, BASH_COMPLETION).unwrap();
        let local_path = fixture.join("download result.txt");
        std::fs::write(&local_path, b"fixture").unwrap();
        let partial_path = fixture.join("download").to_string_lossy().into_owned();

        let output = Command::new("bash")
            .args([
                "-c",
                r#"
source "$1"
COMP_WORDS=("pikpaktui" "download" "-o" "$2")
COMP_CWORD=3
_pikpaktui
printf '<%s>\n' "${COMPREPLY[@]}"
"#,
                "completion-test",
            ])
            .arg(&completion_path)
            .arg(&partial_path)
            .output()
            .unwrap();

        let _ = std::fs::remove_dir_all(&fixture);
        assert!(
            output.status.success(),
            "bash completion failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("<{}>\n", local_path.display())
        );
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
