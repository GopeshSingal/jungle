_jungle__jungle_cmd_complete() {
  local cur="${COMP_WORDS[COMP_CWORD]}"
  COMPREPLY=($(compgen -W "$(command jungle aliases --names 2>/dev/null)" -- "$cur"))
}
# bash splits words on '@' by default, which would break repo@branch completion
COMP_WORDBREAKS=${COMP_WORDBREAKS//@/}
complete -o default -F _jungle__jungle_cmd_complete __jungle_cmd
