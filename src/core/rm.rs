use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::core::aliases::{self, Outcome};
use crate::core::config::Config;
use crate::core::git;
use crate::core::model::{Index, Repo, Worktree};

/// remove a worktree by alias and return its path.
pub fn remove(cfg: &Config, idx: &Index, query: &str, force: bool) -> Result<PathBuf> {
    let path = match aliases::resolve(idx, cfg, query) {
        Outcome::Found(path) => path,
        Outcome::Ambiguous(candidates) => {
            let mut msg = format!("'{query}' matches more than one place:");
            for (alias, path) in candidates {
                msg.push_str(&format!("\n  {alias:<32} {}", path.display()));
            }
            bail!("{msg}");
        }
        Outcome::NotFound => bail!("nothing matches '{query}'"),
    };

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

/// apply prune: remove every removable worktree, prune stale registrations.
/// returns paths that were removed.
pub fn prune_apply(idx: &Index) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for repo in &idx.repos {
        let default = repo.default_branch.as_deref();
        let targets: Vec<&Worktree> = repo
            .worktrees
            .iter()
            .filter(|w| w.removable(default))
            .collect();
        for wt in targets {
            remove_worktree(repo, wt, false)?;
            removed.push(wt.path.clone());
        }
        let stale = repo.worktrees.iter().any(|w| w.prunable);
        if stale {
            git::worktree_prune(&repo.home())?;
        }
    }
    Ok(removed)
}
