use std::path::{PathBuf};

use anyhow::{bail, Context, Result};

use crate::core::aliases;
use crate::core::config::Config;
use crate::core::git;
use crate::core::model::{Index, Repo, Worktree};

/// remove a worktree by alias and return its path.
pub fn remove(cfg: &Config, idx: &Index, query: &str, force: bool) -> Result<PathBuf> {
    let path = aliases::resolve_one(idx, cfg, query)?;

    let (repo, wt) = idx
        .worktree_at(&path)
        .with_context(|| format!("no worktree at {}", path.display()))?;

    remove_worktree(repo, wt, force)?;
    Ok(wt.path.clone())
}

/// remove a known worktree. `force` is required when it is not `removable`.
pub fn remove_worktree(repo: &Repo, wt: &Worktree, force: bool) -> Result<()> {
    if wt.is_main {
        bail!("refusing to remove the main worktree at {}", wt.path.display());
    }

    let default = repo.default_branch.as_deref();
    if !wt.removable(default) && !force {
        bail!("{}", refuse_reasons(wt, default));
    }

    git::worktree_remove(&repo.home(), &wt.path, force)
}

fn refuse_reasons(wt: &Worktree, default_branch: Option<&str>) -> String {
    let mut reasons: Vec<String> = Vec::new();
    if wt.locked {
        reasons.push("locked".into());
    }
    if wt.prunable {
        reasons.push("missing on disk".into());
    }
    if !wt.merged() {
        reasons.push("not merged into the default branch".into());
    }
    for r in wt.risks(default_branch) {
        reasons.push(r.describe().into());
    }
    if reasons.is_empty() {
        reasons.push("not safe to remove".into());
    }
    format!(
        "worktree at {} is not safe to remove ({}). Pass --force to remove anyway.",
        wt.path.display(),
        reasons.join(", ")
    )
}

#[derive(Default)]
pub struct Pruned {
    pub removed:      Vec<PathBuf>,
    pub failed:       Vec<(PathBuf, String)>,  // worktrees that couldn't be removed
    pub prune_failed: Vec<(PathBuf, String)>,  // repos whose stale registrations couldn't be pruned
}

/// apply prune: remove every removable worktree, prune stale registrations
pub fn prune_apply(idx: &Index) -> Pruned {
    let mut out = Pruned::default();
    for repo in &idx.repos {
        let default = repo.default_branch.as_deref();
        let targets: Vec<&Worktree> = repo
            .worktrees
            .iter()
            .filter(|w| w.removable(default))
            .collect();
        for wt in targets {
            match remove_worktree(repo, wt, false) {
                Ok(())   => out.removed.push(wt.path.clone()),
                Err(err) => out.failed.push((wt.path.clone(), err.to_string())),
            }
        }
        let stale = repo.worktrees.iter().any(|w| w.prunable);
        if stale {
            if let Err(err) = git::worktree_prune(&repo.home()) {
                out.prune_failed.push((repo.home(), err.to_string()));
            }
        }
    }
    out
}
