use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub generated_at: u64,          // Unix time of the last discovery call
    pub status_at:    Option<u64>,  // Unix time of the last status refresh
    pub repos:        Vec<Repo>,
}

impl Index {
    pub fn worktree_at(&self, path: &Path) -> Option<(&Repo, &Worktree)> {
        for repo in &self.repos {
            if let Some(wt) = repo.worktrees.iter().find(|w| w.path == path) {
                return Some((repo, wt));
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub name:           String,
    pub common_dir:     PathBuf,
    pub main_path:      Option<PathBuf>, // the main worktree
    pub default_branch: Option<String>,  // short name of the default branch
    pub default_ref:    Option<String>,  // ref used for merge checks
    pub worktrees:      Vec<Worktree>,
}

impl Repo {
    // where `j <repo>` redirects: the main worktree or the directory holding the bare repo
    pub fn home(&self) -> PathBuf {
        if let Some(p) = &self.main_path {
            return p.clone();
        }
        if self.common_dir.file_name() == Some(OsStr::new(".bare")) {
            if let Some(parent) = self.common_dir.parent() {
                return parent.to_path_buf();
            }
        }
        self.common_dir.clone()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.home() == path || self.worktrees.iter().any(|w| w.path == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Head {
    Branch(String),
    Detached(String),
    Unknown,
}

impl Default for Head {
    fn default() -> Self {
        Head::Unknown
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Worktree {
    pub path:       PathBuf,
    pub name:       String,
    pub head:       Head,
    pub is_main:    bool,
    pub locked:     bool,
    pub prunable:   bool,
    pub status:     Option<Status>,
    pub disk_bytes: Option<u64>,
    #[serde(default)]
    pub sessions:   Vec<Session>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub agent:          String,
    pub id:             String,
    #[serde(alias = "last_prompt")]
    pub summary:        Option<String>,  // last prompt or thread title (depends on agent)
    pub last_active_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub dirty:           bool,
    pub untracked:       Option<bool>,
    pub upstream:        Option<String>,
    pub ahead_upstream:  Option<u32>,
    pub behind_upstream: Option<u32>,
    pub ahead_main:      Option<u32>,
    pub behind_main:     Option<u32>,
    pub merged:          Option<bool>,
    pub last_commit_at:  Option<u64>,
}

// the four ways a worktree can hold work that exists nowhere else
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Uncommitted,
    Untracked,
    Unpushed,
    LocalOnly,
}

impl Risk {
    pub fn chip(self) -> &'static str {
        match self {
            Risk::Uncommitted => "dirty",
            Risk::Untracked   => "untracked",
            Risk::Unpushed    => "unpushed",
            Risk::LocalOnly   => "local-only"
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Risk::Uncommitted => "uncommitted changes",
            Risk::Untracked   => "untracked files",
            Risk::Unpushed    => "commits not pushed to its upstream",
            Risk::LocalOnly   => "branch has no upstream and is not merged"
        }
    }
}

impl Worktree {
    pub fn branch(&self) -> Option<&str> {
        match &self.head {
            Head::Branch(b) => Some(b.as_str()),
            _               => None,
        }
    }

    pub fn merged(&self) -> bool {
        self.status.as_ref().and_then(|s| s.merged) == Some(true)
    }

    pub fn risks(&self, default_branch: Option<&str>) -> Vec<Risk> {
        let Some(s) = &self.status else {
            return Vec::new();
        };
        let mut risks = Vec::new();
        if s.dirty {
            risks.push(Risk::Uncommitted)
        }
        if s.untracked == Some(true) {
            risks.push(Risk::Untracked)
        }
        if s.ahead_upstream.unwrap_or(0) > 0 {
            risks.push(Risk::Unpushed)
        }
        let on_default = default_branch.is_some() && self.branch() == default_branch;
        let has_own_commits = s.ahead_main.map_or(true, |n| n > 0);
        if s.upstream.is_none() && !on_default && s.merged != Some(true) && has_own_commits {
            risks.push(Risk::LocalOnly);
        }
        risks
    }

    // is this safe to suggest to be removed? (a linked worktree, merged, with nothing at risk)
    pub fn removable(&self, default_branch: Option<&str>) -> bool {
        !self.is_main
            && !self.locked
            && !self.prunable
            && self.merged()
            && self.risks(default_branch).is_empty()
    }

    pub fn last_session(&self) -> Option<&Session> {
        self.sessions.first()
    }
}
