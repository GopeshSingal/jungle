_jungle__jungle_cmd_complete() {
  local -a targets
  targets=(${(f)"$(command jungle aliases --names 2>/dev/null)"})
  compadd -a targets
}
if (( $+functions[compdef] )); then
  compdef _jungle__jungle_cmd_complete __jungle_cmd
fi
