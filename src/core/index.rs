use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::core::config::{cache_dir, Config};
use crate::core::git::default_ref;
use crate::core::model::{Index, Repo, Worktree};
use crate::core::{discover, git};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn path() -> PathBuf {
    cache_dir().join("index.json")
}

pub fn load() -> Option<Index> {
    let s = fs::read_to_string(path()).ok()?;
    serde_json::from_str(&s).ok()
}

// writes to a tmp file and renames it
pub fn save(idx: &Index) -> Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(idx)?).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &p).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

// discovery plus status for every worktree
pub fn full_scan(cfg: &Config, disk: bool, untracked: bool) -> Index {
    let mut repos = discover::discover(cfg);
    refresh_status(&mut repos, disk, untracked);
    let t = now();
    Index {
        generated_at: t,
        status_at:    Some(t),
        repos,
    }
}

// discovery only
pub fn rediscover(cfg: &Config, previous: Option<&Index>) -> Index {
    let mut repos = discover::discover(cfg);
    let mut status_at = None;
    if let Some(prev) = previous {
        status_at = prev.status_at;
        let mut old_repos: HashMap<&Path, &Repo> = HashMap::new();
        let mut old_worktrees: HashMap<&Path, &Worktree> = HashMap::new();
        for r in &prev.repos {
            old_repos.insert(r.common_dir.as_path(), r);
            for w in &r.worktrees {
                old_worktrees.insert(w.path.as_path(), w);
            }
        }
        for r in &mut repos {
            if let Some(old) = old_repos.get(r.common_dir.as_path()) {
                r.default_branch = old.default_branch.clone();
                r.default_ref = old.default_ref.clone();
            }
            for w in &mut r.worktrees {
                if let Some(old) = old_worktrees.get(w.path.as_path()) {
                    w.status = old.status.clone();
                    w.disk_bytes = old.disk_bytes;
                }
            }
        }
    }
    Index {
        generated_at: now(),
        status_at,
        repos,
    }
}

// the cached index, used by `j` and completion
pub fn load_fresh(cfg: &Config, max_age_secs: u64) -> Index {
    let cached = load();
    let fresh = cached
        .as_ref()
        .is_some_and(|i| now().saturating_sub(i.generated_at) <= max_age_secs);
    if fresh {
        return cached.unwrap();
    }
    let idx = rediscover(cfg, cached.as_ref());
    let _ = save(&idx);
    idx
}

pub fn load_or_scan(cfg: &Config, force: bool, disk: bool, untracked: bool) -> Result<Index> {
    if !force {
        if let Some(idx) = load().filter(|i| i.status_at.is_some()) {
            return Ok(idx);
        }
    }
    let idx = full_scan(cfg, disk, untracked);
    save(&idx)?;
    Ok(idx)
}

fn refresh_status(repos: &mut [Repo], disk: bool, untracked: bool) {
    repos.par_iter_mut().for_each(|repo| {
        let default = git::default_ref(&repo.common_dir);
        repo.default_ref = default.as_ref().map(|d| d.0.clone());
        repo.default_branch = default.map(|d| d.1);
        let default_ref = repo.default_ref.clone();
        let default_branch = repo.default_branch.clone();

        repo.worktrees.par_iter_mut().for_each(|wt| {
            if wt.prunable {
                return;
            }
            let on_default = default_branch.is_some() && wt.branch() == default_branch.as_deref();
            wt.status = git::status(&wt.path, default_ref.as_deref(), on_default, untracked);
            if disk {
                // TODO: implement disk usage
            }
        });
    })
}
