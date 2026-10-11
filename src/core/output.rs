use std::path::Path;

use crate::core::aliases::{repo_names, worktree_alias};
use crate::core::config::home;
use crate::core::index::now;
use crate::core::model::{Index, Risk, Status, Worktree};

pub struct Filter {
    pub merged: bool,
    pub risk:   bool,
}

pub fn table(idx: &Index, filter: &Filter) {
    let now = now();
    let names = repo_names(idx);
    let mut rows: Vec<[String; 6]> = Vec::new();
    for (repo, name) in idx.repos.iter().zip(&names) {
        let default = repo.default_branch.as_deref();
        for wt in &repo.worktrees {
            let risks = wt.risks(default);
            if filter.merged && !wt.merged() {
                continue;
            }
            if filter.risk && risks.is_empty() {
                continue;
            }
            let last = wt.
                status
                .as_ref()
                .and_then(|s| s.last_commit_at)
                .map(|t| format!("{} ago", age(now.saturating_sub(t))))
                .unwrap_or_default();
            rows.push([
                worktree_alias(name, wt),
                state(wt, &risks),
                vs_main(wt),
                last,
                wt.disk_bytes.map(bytes).unwrap_or_default(),
                tilde(&wt.path),
            ]);
        }
    }

    println!("{}", summary(idx));
    if rows.is_empty() {
        println!("No worktrees match.");
        return;
    }
    println!();
    print_rows(["ALIAS", "STATE", "VS_MAIN", "LAST COMMIT", "DISK", "PATH"], &rows);
}

pub fn summary(idx: &Index) -> String {
    let mut worktrees = 0;
    let mut at_risk = 0;
    let mut removeable = 0;
    let mut reclaimable: Option<u64> = None;
    for repo in &idx.repos {
        let default = repo.default_branch.as_deref();
        for wt in &repo.worktrees {
            worktrees += 1;
            if !wt.risks(default).is_empty() {
                at_risk += 1;
            }
            if wt.removable(default) {
                removeable += 1;
                if let Some(b) = wt.disk_bytes {
                    *reclaimable.get_or_insert(0) += b;
                }
            }
        }
    }
    let mut line = format!(
        "{} repos - {} worktrees - {} at risk - {} merged and clean",
        idx.repos.len(),
        worktrees,
        at_risk,
        removeable
    );
    if let Some(b) = reclaimable {
        line.push_str(&format!(" - {} reclaimable", bytes(b)));
    }
    match idx.status_at {
        Some(t) => line.push_str(&format!(" - status from {} ago", age(now().saturating_sub(t)))),
        None    => line.push_str(" - no status yet (run `jungle scan`)"),
    }
    line
}

pub fn risk(idx: &Index) {
    let names = repo_names(idx);
    let mut count = 0;
    let mut checked_untracked = false;
    for (repo, name) in idx.repos.iter().zip(&names) {
        let default = repo.default_branch.as_deref();
        for wt in &repo.worktrees {
            if wt.status.as_ref().is_some_and(|s| s.untracked.is_some()) {
                checked_untracked = true;
            }
            let risks = wt.risks(default);
            if risks.is_empty() {
                continue;
            }
            count += 1;
            println!("{}  {}", worktree_alias(name, wt), tilde(&wt.path));
            for r in risks {
                println!("    {}", r.describe());
            }
        }
    }
    if count == 0 {
        println!("Nothing at risk: all work is committed and pushed.");
    } else {
        println!();
        println!("{count} worktrees have work that only exists on this machine.");
    }
    if !checked_untracked {
        println!("Untracked files weren't checked. Run `jungle scan --untracked` to include them.");
    }
}

pub fn prune(idx: &Index) {
    println!("# Merged, clean worktrees. Review these, then run them (for example `jungle prune | sh`).");
    println!("# jungle can't yet tell whether something is running inside a worktree. Check first.");

    let mut count = 0;
    for repo in &idx.repos {
        let default = repo.default_branch.as_deref();
        let at = shell_quote(&repo.home().to_string_lossy());
        for wt in repo.worktrees.iter().filter(|w| w.removable(default)) {
            count += 1;
            println!("git -C {at} worktree remove {}", shell_quote(&wt.path.to_string_lossy()));
            if let Some(b) = wt.branch() {
                println!("# git -C {at} worktree prune -d {}", shell_quote(b));
            }
        }
        let stale = repo.worktrees.iter().filter(|w| w.prunable).count();
        if stale > 0 {
            println!("git -C {at} worktree prune  # {stale} missing worktrees(s)");
        }
    }
    if count == 0 {
        println!("# Nothing merged and clean to remove.");
    }
}

fn state(wt: &Worktree, risks: &[Risk]) -> String {
    let mut chips: Vec<&str> = Vec::new();
    if wt.is_main {
        chips.push("main");
    }
    if wt.prunable {
        chips.push("missing");
    }
    if wt.locked {
        chips.push("locked");
    }
    chips.extend(risks.iter().map(|r| r.chip()));
    if wt.merged() {
        chips.push("merged");
    }
    if wt.status.is_none() && !wt.prunable {
        chips.push("?");
    }
    if chips.is_empty() {
        "clean".to_string()
    } else {
        chips.join(" ")
    }
}

fn vs_main(wt: &Worktree) -> String {
    match &wt.status {
        Some(Status {
            ahead_main:  Some(a),
            behind_main: Some(b),
            ..
        }) => format!("+{a} -{b}"),
        _  => String::new(),
    }
}

fn print_rows<const N: usize>(header: [&str; N], rows: &[[String; N]]) {
    let mut widths = [0usize; N];
    for (i, h) in header.iter().enumerate() {
        widths[i] = h.chars().count();
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let print = |cells: &[&str]| {
        let mut line = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == N {
                line.push_str(cell);
            } else {
                line.push_str(&format!("{:<width$}  ", cell, width = widths[i]));
            }
        }
        println!("{}", line.trim_end());
    };
    print(&header);
    for row in rows {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        print(&cells);
    }
}

pub fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_)   => p.display().to_string(),
    }
}

fn age(secs: u64) -> String {
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;
    match secs {
        s if s < HOUR     => format!("{}m", s / 60),
        s if s < DAY      => format!("{}h", s / HOUR),
        s if s < 14 * DAY => format!("{}d", s / DAY),
        s if s < 60 * DAY => format!("{}w", s / (7 * DAY)),
        s => format!("{}mo", s / (30 * DAY)),
    }
}

fn bytes(n: u64) -> String {
    const K: f64 = 1024.0;
    let n = n as f64;
    if n >= K * K * K {
        format!("{:.1} GB", n / (K * K * K))
    } else if n >= K * K {
        format!("{:.0} MB", n / (K * K))
    } else {
        format!("{:.0} KB", n / K)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}
