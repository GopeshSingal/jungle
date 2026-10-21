use std::io::{self, IsTerminal};
use std::path::Path;

use crate::core::aliases::{repo_names, worktree_alias};
use crate::core::config::{config_path, expand, home, Config};
use crate::core::index::{self, now};
use crate::core::model::{Index, Merge, Risk, Status, Worktree};

const RECENT_SESSION_SECS: u64 = 3600;

pub struct Filter {
    pub merged: bool,
    pub risk:   bool,
}

pub fn table(idx: &Index, filter: &Filter) {
    let now = now();
    let names = repo_names(idx);
    let mut rows: Vec<[String; 8]> = Vec::new();
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
                agent(wt, now),
                note_preview(wt.note.as_deref()),
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
    print_rows(["ALIAS", "STATE", "VS_MAIN", "LAST COMMIT", "DISK", "AGENT", "NOTE", "PATH"], &rows);
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

// agent sessions per worktree
pub fn sessions(idx: &Index, only: Option<&Path>) {
    let now = now();
    let names = repo_names(idx);
    let mut count = 0;
    for (repo, name) in idx.repos.iter().zip(&names) {
        for wt in &repo.worktrees {
            if wt.sessions.is_empty() || only.is_some_and(|p| p != wt.path) {
                continue;
            }
            println!("{}  {}", worktree_alias(name, wt), tilde(&wt.path));
            for s in &wt.sessions {
                count += 1;
                let ago = format!("{} ago", age(now.saturating_sub(s.last_active_at)));
                let id: String = s.id.chars().take(8).collect();
                let prompt = s.summary.as_deref().map(|p| clip(p, 70)).unwrap_or_default();
                println!("   {ago:<8} {} {id}  {prompt}", s.agent);
            }
        }
    }
    if count == 0 {
        println!("no agent sessions found");
    } else {
        println!();
        println!("resume the newest with `jungle resume <alias>`");
    }
}

pub fn prune(idx: &Index) {
    println!("# Merged, clean worktrees. Review these, then run `jungle prune --apply`.");
    println!("# jungle can't yet tell whether something is running inside a worktree. Check first.");

    let mut count = 0;
    for repo in &idx.repos {
        let default = repo.default_branch.as_deref();
        let at = shell_quote(&repo.home().to_string_lossy());
        for wt in repo.worktrees.iter().filter(|w| w.removable(default)) {
            count += 1;
            let mut line = format!("git -C {at} worktree remove {}", shell_quote(&wt.path.to_string_lossy()));
            let mut notes: Vec<String> = Vec::new();
            if wt.merge() == Some(Merge::Squash) {
                notes.push("squash-merged".into());
            }
            if let Some(s) = wt.last_session() {
                let idle = now().saturating_sub(s.last_active_at);
                if idle < RECENT_SESSION_SECS {
                    notes.push(format!("{} active {} ago", s.agent, age(idle)));
                }
            }
            if !notes.is_empty() {
                line.push_str(&format!("  # {}", notes.join(", ")));
            }
            println!("{line}");
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

// the settings in effect and where they come from
pub fn config(cfg: &Config) {
    let path = config_path();
    let source = if path.is_file() { "" } else { "  (not found, using defaults)" };
    println!("config     {}{source}", tilde(&path));
    println!("index      {}", tilde(&index::path()));
    println!("max_depth  {}", cfg.max_depth);
    println!("ignore     {}", cfg.ignore.join(" "));

    println!();
    println!("roots");
    let roots: Vec<(String, String)> = cfg
        .roots
        .iter()
        .map(|r| (r.clone(), presence(expand(r).is_dir())))
        .collect();
    print_pairs(&roots);

    println!();
    println!("aliases");
    let aliases: Vec<(String, String)> = cfg
        .alias_paths()
        .into_iter()
        .map(|(name, p)| {
            let missing = if p.exists() { "" } else { "  missing" };
            (name, format!("{}{missing}", tilde(&p)))
        })
        .collect();
    print_pairs(&aliases);
}

pub fn config_json(cfg: &Config) -> serde_json::Value {
    let path = config_path();
    let roots: Vec<serde_json::Value> = cfg
        .roots
        .iter()
        .map(|r| {
            let p = expand(r);
            serde_json::json!({ "root": r, "path": p, "exists": p.is_dir() })
        })
        .collect();
    let aliases: Vec<serde_json::Value> = cfg
        .alias_paths()
        .into_iter()
        .map(|(name, p)| serde_json::json!({ "name": name, "path": p, "exists": p.exists() }))
        .collect();
    serde_json::json!({
        "config":    { "path": path, "exists": path.is_file() },
        "index":     index::path(),
        "max_depth": cfg.max_depth,
        "ignore":    cfg.ignore,
        "roots":     roots,
        "aliases":   aliases,
    })
}

fn presence(exists: bool) -> String {
    if exists { "ok" } else { "missing" }.to_string()
}

fn print_pairs(pairs: &[(String, String)]) {
    if pairs.is_empty() {
        println!("  none");
        return;
    }
    let width = pairs.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    for (k, v) in pairs {
        println!("  {k:<width$}  {v}");
    }
}

fn state(wt: &Worktree, risks: &[Risk]) -> String {
    let color = use_color();
    let mut chips: Vec<String> = Vec::new();
    if wt.is_main {
        chips.push("main".into());
    }
    if wt.prunable {
        chips.push("missing".into());
    }
    if wt.locked {
        chips.push("locked".into());
    }
    for r in risks {
        chips.push(paint_chip(r.chip(), color));
    }
    match wt.merge() {
        Some(Merge::Squash)   => chips.push("squashed".into()),
        Some(Merge::Ancestor) => chips.push("merged".into()),
        _                     => {}
    }
    if wt.status.is_none() && !wt.prunable {
        chips.push("?".into());
    }
    if chips.is_empty() {
        "clean".to_string()
    } else {
        chips.join(" ")
    }
}

fn paint_chip(chip: &str, color: bool) -> String {
    if color && chip == "dirty" {
        format!("\x1b[31m{chip}\x1b[0m")
    } else {
        chip.to_string()
    }
}

fn use_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn agent(wt: &Worktree, now: u64) -> String {
    let Some(s) = wt.last_session() else {
        return String::new();
    };
    let mut cell = format!("{} {}", s.agent, age(now.saturating_sub(s.last_active_at)));
    if wt.sessions.len() > 1 {
        cell.push_str(&format!(" +{}", wt.sessions.len() - 1));
    }
    cell
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('-');
    out
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
        widths[i] = visible_len(h);
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(visible_len(cell));
        }
    }
    let print = |cells: &[&str]| {
        let mut line = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == N {
                line.push_str(cell);
            } else {
                let pad = widths[i].saturating_sub(visible_len(cell));
                line.push_str(cell);
                line.extend(std::iter::repeat(' ').take(pad + 2));
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

// column width ignoring ANSI SGR sequences so colored chips still align
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.next_if_eq(&'[').is_some() {
            while let Some(c) = chars.next() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            len += 1;
        }
    }
    len
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

pub fn shell_word(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if plain {
        s.to_string()
    } else {
        shell_quote(s)
    }
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn note_preview(note: Option<&str>) -> String {
    let text = note.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 60 {
        format!("{}…", text.chars().take(59).collect::<String>())
    } else {
        text
    }
}