use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::core::model::{Merge, Status};

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = command_loud(dir, args);
    cmd.stderr(Stdio::null());
    cmd
}

fn command_loud(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")   // stops `git status` from taking `index.lock`
        .env("GIT_TERMINAL_PROMPT", "0")  // stops git from waiting for a password
        .stdin(Stdio::null());
    cmd
}

// runs `git worktree <sub> <args>`, failing with git's stderr
fn worktree(home: &Path, sub: &str, args: &[&str]) -> Result<()> {
    let mut full = vec!["worktree", sub];
    full.extend(args);
    let out = command_loud(home, &full)
        .output()
        .with_context(|| format!("running git worktree {sub} in {}", home.display()))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    if err.is_empty() {
        bail!("git worktree {sub} failed");
    }
    bail!("{err}");
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
            match succeeds(path, &["merge-base", "--is-ancestor", "HEAD", dref]) {
                Some(true) => Some(Merge::Ancestor),
                Some(false) if squash_merged(path, dref) == Some(true) => Some(Merge::Squash),
                Some(false) => Some(Merge::No),
                None => None,
            }
        }
    }
    Some(s)
}

fn squash_merged(path: &Path, dref: &str) -> Option<bool> {
    let base = output(path, &["merge-base", dref, "HEAD"])?;
    let base = base.trim();

    let ours = patch_ids(path, &["diff", "--no-color", "--no-ext-diff", "--no-renames", base, "HEAD"])?;
    let Some(ours) = ours.split_whitespace().next() else {
        return Some(false); // the branch's commits cancel out
    };

    let names = output(path, &["diff", "--no-renames", "--name-only", "-z", base, "HEAD"])?;
    let range = format!("{base}..{dref}");
    let mut args = vec![
        "--literal-pathspecs", "log", "-p", "--no-color", "--no-ext-diff", "--no-renames",
        "--format=commit %H", &range, "--",
    ];
    args.extend(names.split('\0').filter(|n| !n.is_empty()));
    let theirs = patch_ids(path, &args)?;

    Some(theirs.lines().any(|l| l.split_whitespace().next() == Some(ours)))
}

// pipes `git <args>` through `git patch-id --stable`
fn patch_ids(dir: &Path, args: &[&str]) -> Option<String> {
    let mut src = command(dir, args).stdout(Stdio::piped()).spawn().ok()?;
    let pipe = src.stdout.take()?;
    let out = command(dir, &["patch-id", "--stable"]).stdin(pipe).output().ok()?;
    let src_ok = src.wait().ok()?.success();
    (src_ok && out.status.success()).then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// root of the worktree containing dir
pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    let out = output(dir, &["rev-parse", "--show-toplevel"])?;
    Some(PathBuf::from(out.trim_end_matches(['\r', '\n'])))
}

pub fn branch_exists(home: &Path, branch: &str) -> bool {
    let r = format!("refs/heads/{branch}");
    succeeds(home, &["rev-parse", "--verify", "-q", &r]) == Some(true)
}

// `refs/remotes/origin/<branch>` if origin has the branch
pub fn remote_branch(home: &Path, branch: &str) -> Option<String> {
    let r = format!("refs/remotes/origin/{branch}");
    (succeeds(home, &["rev-parse", "--verify", "-q", &r]) == Some(true)).then_some(r)
}

pub fn worktree_add(
    home: &Path,
    path: &Path,
    branch: &str,
    create_new: bool,
    base: Option<&str>,
    track: bool,
) -> Result<()> {
    let path_s = path.to_string_lossy();
    if !create_new {
        return worktree(home, "add", &[path_s.as_ref(), branch]);
    }
    let base = base.context("no base ref to create the branch from (pass --base)")?;
    let mut args = Vec::new();
    if track {
        args.push("--track");
    }
    args.extend(["-b", branch, path_s.as_ref(), base]);
    worktree(home, "add", &args)
}

pub fn worktree_remove(home: &Path, path: &Path, force: bool) -> Result<()> {
    let path_s = path.to_string_lossy();
    let mut args = Vec::new();
    if force {
        args.push("--force");
    }
    args.push(path_s.as_ref());
    worktree(home, "remove", &args)
}

pub fn worktree_prune(home: &Path) -> Result<()> {
    worktree(home, "prune", &[])
}
