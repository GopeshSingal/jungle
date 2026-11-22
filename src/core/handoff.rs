use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{git, index, model::Index, notes};

const FILENAME: &str = "jungle-handoff.json";
const TEMPLATE: &str = "## Next\n\n\n## Done\n\n\n## Verified\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handoff {
    pub saved_at: u64,
    pub commit: Option<String>,
    pub tracked_changes: bool,
    pub untracked_files: bool,
    pub next: String,
    pub markdown: String,
}

pub fn path(dir: &Path) -> Result<PathBuf> {
    notes::worktree_file(dir, FILENAME)
}

pub fn read(path: &Path) -> Result<Option<Handoff>> {
    notes::read(path)?
        .map(|text| {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        })
        .transpose()
}

pub fn head(dir: &Path) -> Result<Option<String>> {
    let branch = git::output(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=no",
        ],
    )
    .context("reading current Git HEAD")?;
    commit(&branch)
}

fn commit(status: &str) -> Result<Option<String>> {
    let oid = status
        .lines()
        .find_map(|line| line.strip_prefix("# branch.oid "))
        .context("Git status did not report HEAD")?;
    Ok((oid != "(initial)").then(|| oid.to_owned()))
}

fn save(dir: &Path, path: &Path, draft: notes::Draft, markdown: &str) -> Result<()> {
    let markdown = markdown.trim();
    let mut lines = markdown.lines();
    let next = lines
        .next()
        .filter(|line| line.trim() == "## Next")
        .and_then(|_| {
            lines
                .map(str::trim)
                .take_while(|line| !line.starts_with('#'))
                .find(|line| !line.is_empty())
        })
        .context("start the handoff with a ## Next heading and a next step")?;
    let status = git::output(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ],
    )
    .context("reading Git status for the handoff")?;
    let handoff = Handoff {
        saved_at: index::now(),
        commit: commit(&status)?,
        tracked_changes: status
            .lines()
            .any(|line| matches!(line.as_bytes().first(), Some(b'1' | b'2' | b'u'))),
        untracked_files: status.lines().any(|line| line.starts_with("? ")),
        next: next.to_owned(),
        markdown: markdown.to_owned(),
    };
    draft.replace(path, &serde_json::to_string_pretty(&handoff)?)
}

pub fn save_text(dir: &Path, path: &Path, markdown: &str) -> Result<()> {
    save(dir, path, notes::Draft::create(path)?, markdown)
}

pub fn edit(dir: &Path, path: &Path) -> Result<()> {
    let existing = read(path)?;
    let initial = existing
        .as_ref()
        .map_or(TEMPLATE, |handoff| handoff.markdown.as_str());
    let draft = notes::Draft::create(path)?;
    let text = draft.edit(dir, initial)?;
    if text.trim() == initial.trim() {
        return Ok(());
    }
    save(dir, path, draft, &text)
}

pub fn attach_metadata(index: &mut Index) {
    for wt in index.repos.iter_mut().flat_map(|repo| &mut repo.worktrees) {
        wt.note = None;
        wt.handoff = None;
        if !wt.prunable {
            if let Ok(note_path) = notes::path(&wt.path) {
                wt.note = notes::read(&note_path).ok().flatten();
                wt.handoff = read(&note_path.with_file_name(FILENAME)).ok().flatten();
            }
        }
    }
}
