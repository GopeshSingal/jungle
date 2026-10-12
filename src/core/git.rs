use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::core::model::Status;

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")   // stops `git status` from taking `index.lock`
        .env("GIT_TERMINAL_PROMPT", "0")  // stops git from waiting for a password
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

fn command_loud(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    cmd
}

fn output(dir: &Path, args: &[&str]) -> Option<String> {
    let out = command(dir, args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn succeeds(dir: &Path, args: &[&str]) -> Option<bool> {
    let status = command(dir, args).stdout(Stdio::null()).status().ok()?;
    match status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _       => None,
    }
}

// the ref to compare branches against
pub fn default_ref(common_dir: &Path) -> Option<(String, String)> {
    if let Some(r) = output(common_dir, &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"]) {
        let r = r.trim();
        let short = r.strip_prefix("refs/remotes/origin/").unwrap_or(r);
        return Some((r.to_string(), short.to_string()));
    }
    for branch in ["main", "master"] {
        let r = format!("refs/heads/{branch}");
        if succeeds(common_dir, &["rev-parse", "--verify", "-q", &r]) == Some(true) {
            return Some((r, branch.to_string()));
        }
    }
    None
}

pub fn status(path: &Path, default_ref: Option<&str>, on_default: bool, untracked: bool) -> Option<Status> {
    let untracked_flag = if untracked {
        "--untracked-files=normal"
    } else {
        "--untracked-files=no"
    };
    let out = output(path, &["status", "--porcelain=v2", "--branch", untracked_flag])?;

    let mut s = Status::default();
    let mut has_untracked = false;
    for line in out.lines() {
        if let Some(upstream) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(upstream.to_string());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut parts = ab.split_whitespace();
            s.ahead_upstream = parts.next().and_then(|a| a.trim_start_matches('+').parse().ok());
            s.behind_upstream = parts.next().and_then(|b| b.trim_start_matches('-').parse().ok());
        } else if line.starts_with("? ") {
            has_untracked = true;
        } else if !line.is_empty() && !line.starts_with('#') {
            s.dirty = true;
        }
    }

    if untracked {
        s.untracked = Some(has_untracked);
    }

    s.last_commit_at = output(path, &["log", "-1", "--format=%ct"]).and_then(|t| t.trim().parse().ok());

    // compare against main
    if let (Some(dref), false) = (default_ref, on_default) {
        if let Some(counts) = output(
            path,
            &["rev-list", "--left-right", "--count", &format!("{dref}...HEAD")],
        ) {
            let mut n = counts.split_whitespace().map(|x| x.parse::<u32>().ok());
            s.behind_main = n.next().flatten();
            s.ahead_main = n.next().flatten();
        }
        let at_tip = s.ahead_main == Some(0) && s.behind_main == Some(0);
        s.merged = if at_tip {
            None
        } else {
            succeeds(path, &["merge-base", "--is-ancestor", "HEAD", dref])
        }
    }
    Some(s)
}

pub fn branch_exists(home: &Path, branch: &str) -> bool {
    let r = format!("refs/heads/{branch}");
    succeeds(home, &["rev-parse", "--verify", "-q", &r]) == Some(true)
}

pub fn worktree_add(
    home: &Path,
    path: &Path,
    branch: &str,
    create_new: bool,
    base: Option<&str>,
) -> Result<()> {
    let path_s = path.to_string_lossy();
    let out = if create_new {
        let base = base.context("no base ref to create the branch from (pass --base)")?;
        command_loud(
            home,
            &["worktree", "add", "-b", branch, path_s.as_ref(), base],
        )
        .output()
        .with_context(|| format!("running git worktree add in {}", home.display()))?
    } else {
        command_loud(home, &["worktree", "add", path_s.as_ref(), branch])
            .output()
            .with_context(|| format!("running git worktree add in {}", home.display()))?
    };
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    if err.is_empty() {
        bail!("git worktree add failed");
    }
    bail!("{err}");
}
