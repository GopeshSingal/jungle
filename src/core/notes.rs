use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("running git to resolve worktree settings")?;
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
    worktree_file(dir, "jungle-note")
}

pub(super) fn worktree_file(dir: &Path, filename: &str) -> Result<PathBuf> {
    let output = git(
        dir,
        &[
            "rev-parse",
            "--is-inside-work-tree",
            "--path-format=absolute",
            "--git-path",
            filename,
        ],
    )?;
    let (inside, path) = output
        .split_once('\n')
        .context("missing worktree metadata path from git")?;
    if inside != "true" {
        bail!("not inside a git working tree");
    }
    Ok(PathBuf::from(path))
}

pub fn read(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn clear(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("clearing note {}", path.display())),
    }
}

pub(super) struct Draft(PathBuf);

impl Draft {
    pub(super) fn create(path: &Path) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let candidate = path.with_file_name(format!(
                "jungle-draft-{}-{}.tmp",
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
        self.replace(path, text)
    }

    pub(super) fn replace(self, path: &Path, text: &str) -> Result<()> {
        fs::write(&self.0, text).context("writing draft")?;
        fs::rename(&self.0, path).with_context(|| format!("saving {}", path.display()))
    }

    pub(super) fn edit(&self, dir: &Path, initial: &str) -> Result<String> {
        let editor = git(dir, &["var", "GIT_EDITOR"])?;
        fs::write(&self.0, initial).context("writing editor draft")?;
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("{editor} \"$@\""))
            .arg("jungle-editor")
            .arg(&self.0)
            .current_dir(dir)
            .status()
            .with_context(|| format!("opening editor {editor}"))?;
        if !status.success() {
            bail!("editor failed ({status})");
        }
        fs::read_to_string(&self.0).context("reading editor draft")
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
    let existing = read(path)?.unwrap_or_default();
    let draft = Draft::create(path)?;
    let text = draft.edit(dir, &existing)?;
    draft.save(path, &text)
}
