mod claude;
mod codex;

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::core::config::{canonical, home};
use crate::core::model::{Repo, Session};

const MAX_PER_WORKTREE: usize = 5;

pub fn attach(repos: &mut [Repo]) {
    let mut targets: Vec<(usize, usize)> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    for (ri, repo) in repos.iter_mut().enumerate() {
        for (wi, wt) in repo.worktrees.iter_mut().enumerate() {
            wt.sessions.clear();
            if !wt.prunable {
                targets.push((ri, wi));
                paths.push(wt.path.clone());
            }
        }
    }

    let mut found: Vec<Vec<Session>> = vec![Vec::new(); targets.len()];
    let collected = [
        claude::collect(&claude::dir(), &paths),
        codex::collect(&codex::dir(), &paths),
    ];
    for (i, session) in collected.into_iter().flatten() {
        found[i].push(session);
    }
    for ((ri, wi), mut sessions) in targets.into_iter().zip(found) {
        sessions.sort_by_key(|s| Reverse(s.last_active_at));
        sessions.truncate(MAX_PER_WORKTREE);
        repos[ri].worktrees[wi].sessions = sessions;
    }
}

pub fn resume_command(s: &Session) -> Option<(String, Vec<String>)> {
    let id = s.id.clone();
    match s.agent.as_str() {
        "claude" => Some(("claude".into(), vec!["--resume".into(), id])),
        "codex"  => Some(("codex".into(), vec!["resume".into(), id])),
        _        => None,
    }
}

// agent's data dir: `$var` when set, else `~/<fallback>`
fn agent_dir(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _                        => home().join(fallback),
    }
}

struct Matcher<'a> {
    paths: &'a [PathBuf],
    canon: HashMap<PathBuf, PathBuf>,
}

impl<'a> Matcher<'a> {
    fn new(paths: &'a [PathBuf]) -> Self {
        Self {
            paths,
            canon: HashMap::new(),
        }
    }

    fn find(&mut self, dir: &Path) -> Option<usize> {
        let dir = self
            .canon
            .entry(dir.to_path_buf())
            .or_insert_with(|| canonical(dir));
        best_match(self.paths, dir)
    }
}

// deepest worktree containing `dir` so a session in a nested 
// worktree isn't credited to surrounding worktree
fn best_match(worktrees: &[PathBuf], dir: &Path) -> Option<usize> {
    worktrees
        .iter()
        .enumerate()
        .filter(|(_, w)| dir.starts_with(w))
        .max_by_key(|(_, w)| w.components().count())
        .map(|(i, _)| i)
}
