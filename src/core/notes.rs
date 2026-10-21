use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};

use crate::core::model::Index;

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("running git to resolve worktree note settings")?;
    if !output.status.success() {
        bail!(
            "{}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}

pub fn path(dir: &Path) -> Result<PathBuf> {
    let output = git(
        dir,
        &[
            "rev-parse",
            "--is-inside-work-tree",
            "--path-format=absolute",
            "--git-path",
            "jungle-note",
        ],
    )?;
    let (inside, path) = output
        .split_once('\n')
        .context("missing note path from git")?;
    if inside != "true" {
        bail!("not inside a git working tree");
    }
    Ok(PathBuf::from(path))
}

pub fn read(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading note {}", path.display())),
    }
}

pub fn clear(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("clearing note {}", path.display())),
    }
}

struct Draft(PathBuf);

impl Draft {
    fn create(path: &Path) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let candidate = path.with_file_name(format!(
                "jungle-note-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&candidate) {
                Ok(_) => return Ok(Self(candidate)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).context("creating note draft"),
            }
        }
    }

    fn save(self, path: &Path, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() {
            bail!("empty note; existing note unchanged (use --clear to remove it)");
        }
        fs::write(&self.0, text).context("writing note draft")?;
        fs::rename(&self.0, path).with_context(|| format!("saving note {}", path.display()))
    }
}

impl Drop for Draft {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn save(path: &Path, text: &str) -> Result<()> {
    Draft::create(path)?.save(path, text)
}

pub fn edit(dir: &Path, path: &Path) -> Result<()> {
    let editor = git(dir, &["var", "GIT_EDITOR"])?;
    let existing = read(path)?.unwrap_or_default();
    let draft = Draft::create(path)?;
    fs::write(&draft.0, existing).context("writing note draft")?;
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg("jungle-note")
        .arg(&draft.0)
        .current_dir(dir)
        .status()
        .with_context(|| format!("opening note editor {editor}"))?;
    if !status.success() {
        bail!("note editor failed ({status}); existing note unchanged");
    }
    let text = fs::read_to_string(&draft.0).context("reading edited note")?;
    draft.save(path, &text)
}

pub fn attach(idx: &mut Index) -> Result<()> {
    for repo in &mut idx.repos {
        for wt in &mut repo.worktrees {
            // a note that can't be read shouldn't break listing the rest
            wt.note = if wt.prunable {
                None
            } else {
                path(&wt.path).ok().and_then(|p| read(&p).ok().flatten())
            };
        }
    }
    Ok(())
}
