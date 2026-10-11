# jungle shell integration
__jungle_cmd() {
    local dest
    if [ "$#" -eq 0 ]; then
        if ! command -v fzf >/dev/null 2>&1; then
            command jungle aliases
            return
        fi
        dest=$(command jungle aliases | fzf --delimiter="\t" --with-nth=1 --height=40% --reverse --prompt='__jungle_cmd ' | cut -f2)
        [ -n "$dest" ] || return
    elif [ "$#" -eq 1 ] && [ "$1" = "-" ]; then
        builtin cd -
        return
    elif [ "$#" -eq 1 ] && [ -d "$1" ]; then
        builtin cd -- "$1"
        return
    else
        dest=$(command jungle resolve -- "$@") || return
    fi
    builtin cd -- "$dest"
}
