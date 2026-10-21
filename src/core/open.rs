use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

use super::config::Config;

pub fn open(cfg: &Config, path: &Path) -> Result<()> {
    if !path.is_dir() {
        bail!("{} is not an existing directory; run `jungle scan`", path.display());
    }
    let editor = cfg.editor.clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("VISUAL").ok().filter(|s| !s.trim().is_empty()))
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.trim().is_empty()))
        .context("no editor configured; set `editor` in Jungle's config, or set VISUAL or EDITOR")?;
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg("jungle-open")
        .arg(path)
        .current_dir(path)
        .status()
        .with_context(|| format!("opening editor {editor}"))?;
    if !status.success() {
        bail!("editor {editor} failed ({status})");
    }
    Ok(())
}
