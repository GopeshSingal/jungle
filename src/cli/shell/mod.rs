// shell integration. a child process can't change its parent's shell
// directory, so `j` is a shell function that asks `jungle resolve` for
// a path and runs `cd` itself

use anyhow::{bail, Result};
use clap::ValueEnum;

#[derive(Clone, Copy, ValueEnum)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

const PLACEHOLDER: &str = "__jungle_cmd";

const POSIX_FUNCTION: &str = include_str!("posix.sh");
const ZSH_COMPLETION: &str = include_str!("completion.zsh");
const BASH_COMPLETION: &str = include_str!("completion.bash");
const FISH: &str = include_str!("jungle.fish");

pub fn init(shell: Shell, cmd: &str) -> Result<String> {
    if cmd.is_empty() || !cmd.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("--cmd must be a plain command name, got {cmd:?}");
    }
    let script = match shell {
        Shell::Zsh  => format!("{POSIX_FUNCTION}\n{ZSH_COMPLETION}"),
        Shell::Bash => format!("{POSIX_FUNCTION}\n{BASH_COMPLETION}"),
        Shell::Fish => FISH.to_string(),
    };
    Ok(script.replace(PLACEHOLDER, cmd))
}
