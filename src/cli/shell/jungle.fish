function __jungle_cmd
    set -l dest
    if test (count $argv) -eq 0
        if not type -q fzf
            command jungle aliases
            return
        end
        set dest (command jungle aliases | fzf --delimiter='\t' --with-nth=1 --height=40% --reverse --prompt='__jungle_cmd ' | cut -f2)
        test -n "$dest"; or return
    else if test (count $argv) -eq 1; and test "$argv[1]" = "-"
        cd -
        return
    else if test (count $argv) -eq 1; and test -d "$argv[1]"
        cd $argv[1]
        return
    else
        set dest (command jungle resolve -- $argv); or return
    end
    cd $dest
end

complete -c __jungle_cmd -f -a '(command jungle aliases --names 2>/dev/null)'
