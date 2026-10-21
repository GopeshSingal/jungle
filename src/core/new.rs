use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::core::aliases::{self, Outcome};
use crate::core::config::{canonical, expand, Config};
use crate::core::discover;
use crate::core::git;
use crate::core::model::{Index, Repo};

pub struct Args {
    pub repo:  Option<String>,
    pub branch: String,
    pub path:  Option<PathBuf>,
    pub base:  Option<String>,
}

/// create a linked worktree and return its absolute path.
pub fn create(cfg: &Config, idx: &Index, args: Args) -> Result<PathBuf> {
    let repo = pick_repo(cfg, idx, args.repo.as_deref())?;
    let branch = args.branch.trim();
    if branch.is_empty() {
        bail!("branch name is empty");
    }

    if let Some(existing) = repo
        .worktrees
        .iter()
        .find(|w| !w.prunable && w.branch() == Some(branch))
    {
        bail!(
            "branch '{branch}' is already checked out at {}",
            existing.path.display()
        );
    }

    let home = repo.home();
    let path = match args.path {
        Some(p) => {
            let p = if p.starts_with("~") {
                expand(&p.to_string_lossy())
            } else if p.is_relative() {
                env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(p)
            } else {
                p
            };
            p
        }
        None => default_path(&home, branch),
    };

    if path.exists() {
        bail!("path already exists: {}", path.display());
    }

    ensure_jungle_dir(&home)?;

    let create_new = !git::branch_exists(&home, branch);
    // a branch that only exists on origin is checked out tracking it, unless --base says otherwise
    let remote = match (create_new, &args.base) {
        (true, None) => git::remote_branch(&home, branch),
        _            => None,
    };
    let track = remote.is_some();
    let base = if create_new {
        Some(match args.base.or(remote) {
            Some(b) => b,
            None => {
                let (full, _) = git::default_ref(&repo.common_dir).with_context(|| {
                    format!(
                        "could not find a default branch for {} (pass --base)",
                        home.display()
                    )
                })?;
                full
            }
        })
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    git::worktree_add(&home, &path, branch, create_new, base.as_deref(), track)?;
    Ok(canonical(&path))
}

fn default_path(home: &Path, branch: &str) -> PathBuf {
    home.join(".jungle").join(sanitize_branch(branch))
}

fn sanitize_branch(branch: &str) -> String {
    branch.replace('/', "-")
}

fn ensure_jungle_dir(home: &Path) -> Result<()> {
    let dir = home.join(".jungle");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let gi = dir.join(".gitignore");
    if !gi.exists() {
        fs::write(&gi, "*\n").with_context(|| format!("writing {}", gi.display()))?;
    }
    Ok(())
}

fn pick_repo<'a>(cfg: &Config, idx: &'a Index, query: Option<&str>) -> Result<&'a Repo> {
    match query {
        Some(q) => repo_from_query(cfg, idx, q),
        None    => repo_from_cwd(idx),
    }
}

fn repo_from_cwd(idx: &Index) -> Result<&Repo> {
    let cwd = env::current_dir().context("reading current directory")?;
    let Some(common) = discover::common_dir_of(&cwd) else {
        bail!("not inside a git repo (pass a repo name)");
    };
    idx.repos
        .iter()
        .find(|r| r.common_dir == common)
        .with_context(|| {
            format!(
                "repo at {} is not in the jungle index (run `jungle scan`)",
                common.display()
            )
        })
}

fn repo_from_query<'a>(cfg: &Config, idx: &'a Index, query: &str) -> Result<&'a Repo> {
    match aliases::resolve(idx, cfg, query) {
        Outcome::Found(path) => idx
            .repos
            .iter()
            .find(|r| r.contains(&path))
            .with_context(|| format!("no repo matches '{query}'")),
        Outcome::Ambiguous(candidates) => {
            let mut msg = format!("'{query}' matches more than one place:");
            for (alias, path) in candidates {
                msg.push_str(&format!("\n  {alias:<32} {}", path.display()));
            }
            bail!("{msg}");
        }
        Outcome::NotFound => bail!("nothing matches '{query}'"),
    }
}
