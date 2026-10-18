# jungle

A command-line tool for managing Git worktrees across repositories on your machine. Find worktrees, jump between them, inspect local work, and resume Claude or Codex sessions.

## Install

Requires Rust and Cargo, plus Git on your `PATH`.

```sh
cargo install --path . --locked
```

Make sure Cargo's binary directory (usually `~/.cargo/bin`) is on your `PATH`.

## Configure

Create `~/.config/jungle/config.toml` (or `$XDG_CONFIG_HOME/jungle/config.toml`):

```toml
roots = ["~/code", "~/work"]
max_depth = 3

[aliases]
# Optional shortcuts to repositories or worktrees:
# app = "~/code/my-app"
```

Without a config file, Jungle searches `~/code`, `~/work`, `~/src`, and `~/projects`. Discovery skips hidden directories and common dependency and build directories. Once a repository is found, Jungle loads its linked worktrees too.

## Get started

```sh
jungle scan
jungle ls
jungle new feature/my-change    # Run inside a repository
jungle note -m "Next: finish the parser"
```

To jump between worktrees, add the appropriate line to your shell config:

```sh
# ~/.zshrc
eval "$(jungle init zsh)"

# ~/.bashrc
eval "$(jungle init bash)"

# ~/.config/fish/config.fish
jungle init fish | source
```

Restart your shell or reload its config, then use `j <alias>` to jump to a worktree. `j -` returns to the previous directory. With no arguments, `j` opens a picker when `fzf` is installed, or lists aliases otherwise. Use `jungle aliases` to see available names.

## Commands

| Command | Purpose |
| --- | --- |
| `jungle scan` | Rediscover repositories and refresh worktree status. |
| `jungle ls` | List worktrees; supports `--json`, `--merged`, and `--risk`. |
| `jungle risk` | Show work that only exists on this machine. |
| `jungle new [repo] <branch>` | Create a worktree, by default under `<repo>/.jungle/`. |
| `jungle rm <alias>` | Remove a linked worktree, with safety checks. |
| `jungle prune` | Preview removals of merged, clean worktrees; `--apply` removes them. |
| `jungle aliases` | List aliases and their paths. |
| `jungle resolve <alias>` | Print a worktree's path. |
| `jungle note` | Edit a local note for the current worktree; supports `-m`, `--show`, and `--clear`. |
| `jungle sessions [alias]` | List Claude and Codex sessions associated with worktrees. |
| `jungle resume <alias>` | Resume a session; supports `--agent`, `--pick`, and `--print`. |
| `jungle init <shell>` | Print shell integration for Zsh, Bash, or Fish. |

Use `jungle <command> --help` for options. Scans omit untracked-file checks and disk usage by default; use `jungle scan --untracked --disk` or `jungle ls --untracked --disk` to include them. Resuming sessions requires the corresponding agent CLI on your `PATH`.
