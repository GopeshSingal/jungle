use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::core::config::Config;
use crate::core::model::{Index, Repo, Worktree};

pub enum Outcome {
    Found(PathBuf),
    Ambiguous(Vec<(String, PathBuf)>),
    NotFound,
}

// display name per repo in index order
// repos that share names get parent directory prefix-ed
pub fn repo_names(idx: &Index) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in &idx.repos {
        *counts.entry(r.name.as_str()).or_default() += 1;
    }
    idx.repos
        .iter()
        .map(|r| {
            if counts[r.name.as_str()] < 2 {
                return r.name.clone();
            }
            let home = r.home();
            match home.parent().and_then(|p| p.file_name()) {
                Some(parent) => format!("{}/{}", parent.to_string_lossy(), r.name),
                None         => r.name.clone(),
            }
        })
    .collect()
}

pub fn worktree_alias(repo_name: &str, wt: &Worktree) -> String {
    if wt.is_main {
        return repo_name.to_string();
    }
    match wt.branch() {
        Some(b) => format!("{repo_name}@{b}"),
        None    => format!("{repo_name}@{}", wt.name),
    }
}

// listed aliases -> target, prioritizing user aliases
pub fn all(idx: &Index, cfg: &Config) -> Vec<(String, PathBuf)> {
    let mut out = cfg.alias_paths();
    for (repo, name) in idx.repos.iter().zip(repo_names(idx)) {
        out.push((name.clone(), repo.home()));
        for wt in repo.worktrees.iter().filter(|w| !w.is_main && !w.prunable) {
            if let Some(b) = wt.branch() {
                out.push((format!("{name}@{b}"), wt.path.clone()));
            }
            if wt.branch() != Some(wt.name.as_str()) {
                out.push((format!("{name}@{}", wt.name), wt.path.clone()));
            }
        }
    }
    out
}

pub fn resolve(idx: &Index, cfg: &Config, query: &str) -> Outcome {
    let q = query.trim();
    if q.is_empty() {
        return Outcome::NotFound;
    }

    // split on the first `@`
    if let Some((repo_q, branch_q)) = q.split_once('@') {
        return resolve_in_repo(idx, cfg, repo_q, branch_q);
    }

    let all = all(idx, cfg);
    if let Some((_, path)) = all.iter().find(|(alias, _)| alias == q) {
        return Outcome::Found(path.clone());
    }

    // a branch or worktree name alone so `j feat-x` works without the repo
    let names = repo_names(idx);
    let by_branch: Vec<(String, PathBuf)> = idx
        .repos
        .iter()
        .zip(&names)
        .flat_map(move |(repo, name)| {
            repo.worktrees
                .iter()
                .filter(|w| !w.is_main && !w.prunable)
                .filter(move |w| w.branch() == Some(q) || w.name == q)
                .map(move |w| (worktree_alias(name, w), w.path.clone()))
        })
        .collect();
    match by_branch.len() {
        0 => {}
        1 => return Outcome::Found(by_branch[0].1.clone()),
        _ => return Outcome::Ambiguous(by_branch),
    }

    pick(q, &all).into_outcome()
}

pub fn resolve_one(idx: &Index, cfg: &Config, query: &str) -> Result<PathBuf> {
    match resolve(idx, cfg, query) {
        Outcome::Found(path)           => Ok(path),
        Outcome::Ambiguous(candidates) => {
            let mut msg = format!("'{query}' matches more than one place:");
            for (alias, path) in candidates {
                msg.push_str(&format!("\n {alias:<32} {}", path.display()));
            }
            bail!("{msg}");
        }
        Outcome::NotFound              => bail!("nothing matches '{query}'"),
    }
}

fn resolve_in_repo(idx: &Index, cfg: &Config, repo_q: &str, branch_q: &str) -> Outcome {
    let mut repos: Vec<(String, usize)> = repo_names(idx).into_iter().zip(0..).collect();
    for (alias, path) in cfg.alias_paths() {
        if let Some(i) = idx.repos.iter().position(|r| r.contains(&path)) {
            repos.push((alias, i));
        }
    }
    let repo: &Repo = match pick(repo_q, &repos) {
        Pick::One(i)  => &idx.repos[i],
        Pick::Many(c) => {
            let c = c.into_iter().map(|(a, i)| (a, idx.repos[i].home())).collect();
            return Outcome::Ambiguous(c);
        }
        Pick::Zero    => return Outcome::NotFound,
    };

    if branch_q.is_empty() {
        return Outcome::Found(repo.home());
    }

    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    for wt in repo.worktrees.iter().filter(|w| !w.prunable) {
        if let Some(b) = wt.branch() {
            targets.push((b.to_string(), wt.path.clone()));
        }
        targets.push((wt.name.clone(), wt.path.clone()));
    }
    pick(branch_q, &targets).into_outcome()
}

enum Pick<T> {
    One(T),
    Many(Vec<(String, T)>),
    Zero,
}

impl Pick<PathBuf> {
    fn into_outcome(self) -> Outcome {
        match self {
            Pick::One(p) => Outcome::Found(p),
            Pick::Many(c) => Outcome::Ambiguous(c),
            Pick::Zero => Outcome::NotFound,
        }
    }
}

fn pick<T: Clone + PartialEq>(q: &str, items: &[(String, T)]) -> Pick<T> {
    if let Some((_, t)) = items.iter().find(|(k, _)| k == q) {
        return Pick::One(t.clone());
    }
    let ql = q.to_lowercase();
    let mut scored: Vec<(i64, &(String, T))> = items
        .iter()
        .filter_map(|item| score(&ql, &item.0.to_lowercase()).map(|s| (s, item)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1 .0.cmp(&b.1 .0)));

    let Some(&(best_score, best)) = scored.first() else {
        return Pick::Zero;
    };

    let rivals: Vec<(String, T)> = scored
        .iter()
        .filter(|(s, item)| *s == best_score && item.1 != best.1)
        .map(|(_, item)| (*item).clone())
        .collect();
    if rivals.is_empty() {
        return Pick::One(best.1.clone());
    }
    let mut many = vec![best.clone()];
    many.extend(rivals);
    many.truncate(8);
    Pick::Many(many)
}

fn score(q: &str, key: &str) -> Option<i64> {
    let chars: Vec<char> = key.chars().collect();
    let mut score = 0i64;
    let mut next = 0usize;
    let mut prev: Option<usize> = None;
    for qc in q.chars() {
        let pos = (next..chars.len()).find(|&i| chars[i] == qc)?;
        score += 4;
        if pos == 0 {
            score += 16;
        } else if matches!(chars[pos - 1], '/' | '@' | '-' | '_' | '.') {
            score += 8;
        }
        if prev.is_some_and(|p| p + 1 == pos) {
            score += 6;
        }
        prev = Some(pos);
        next = pos + 1;
    }
    score -= (chars.len() as i64) - (q.chars().count() as i64);
    Some(score)
}
