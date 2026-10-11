use std::collections::BTreeSet; // removes duplicates automatically 
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::config::{canonical, Config};
use crate::core::model::{Head, Repo, Worktree};

pub fn discover(cfg: &Config) -> Vec<Repo> {
    let mut common_dirs = BTreeSet::new();
    for root in cfg.root_paths() {
        walk(&root, 0, cfg, &mut common_dirs);
    }
    for (_, path) in cfg.alias_paths() {
        if let Some(common) = common_dir_of(&path) {
            common_dirs.insert(common);
        }
    }

    let mut repos: Vec<Repo> = common_dirs.iter().filter_map(|c| load_repo(c)).collect();
    repos.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.common_dir.cmp(&b.common_dir)));
    repos
}

fn walk(dir: &Path, depth: usize, cfg: &Config, out: &mut BTreeSet<PathBuf>) {
    if let Some(common) = common_dir_of(dir) {
        out.insert(common);
        return;
    }
    if depth >= cfg.max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // skip symlinked directories
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".") || cfg.ignore.iter().any(|i| i.as_str() == &*name) {
            continue;
        }
        walk(&entry.path(), depth + 1, cfg, out);
    }
}

// the shared git directory for the repo or worktree at `dir`.
// a linked worktree resolves to its main repo's git dir, so a worktree
// found under a root pulls in its repo even when the repo is elsewhere
pub fn common_dir_of(dir: &Path) -> Option<PathBuf> {
    let dotgit = dir.join(".git");
    match fs::symlink_metadata(&dotgit) {
        Ok(md) if md.is_dir() => return Some(canonical(&dotgit)),
        Ok(md) if md.is_file  => {
            return read_gitdir_file(&dotgit).and_then(|g| common_dir_from_gitdir(&g));
        }
        _ => {}
    }
    is_git_dir(dir).then(|| canonical(dir))
}

fn common_dir_from_gitdir(gitdir: &Path) -> Option<PathBuf> {
    // linked worktrees have3 a `commondir` file pointing at the shared git dir
    if let Ok(s) = fs.read_to_string(gitdir.join("commondir")) {
        return Some(canonical(&gitdir.join(s.trim())));
    }

    // submodules keep their git dir under `.git/modules/`
    // TODO: untracked for now
    if gitdir.components().any(|c| c.as_os_str() == OsStr::new("modules")) {
        return None;
    }

    is_git_dir(gitdir).then(|| canonical(gitdir))
}

// heuristic to determine if this is a git dir
fn is_git_dir(dir: &Path) -> bool {
    dir.join("HEAD").is_file() && dir.join("objects").is_dir() && dir.join("refs").is_dir()
}

fn read_gitdir_file(file: &Path) -> Option<PathBuf> {
    let s = fs::read_to_string(file).ok()?;
    let target = s.trim().strip_prefix("gitdir:")?.trim();
    Some(file.parent()?.join(target))
}

fn load_repo(common: &Path) -> Option<Repo> {
    if !common.is_dir() {
        return None;
    }

    // determine the main working dir
    let main_path = if common.file_name() == Some(OsStr::new(".git")) {
        common.parent().map(Path::to_path_buf)
    } else {
        None    // likely bare repo -> assigned None
    };
    let mut repo = Repo {
        name:           String::new(),
        common_dir:     common.to_path_buf(),
        main_path,
        default_branch: None,
        default_ref:    None,
        worktrees:      Vec::new(),
    };
    repo.name = repo_name(&repo.home());

    // create and append primary worktree
    if let Some(main) = &repo.main_path {
        repo.worktrees.push(Worktree {
            path: main.clone(),
            name: dir_name(main),
            head: read_head(&common.join("HEAD")),
            is_main: true,
            locked: false,
            prunable: !main.is_dir(),
            status: None,
            disk_bytes: None,
        });
    }

    // look for additional worktrees in `.git/worktrees`
    if let Ok(entries) = fs::read_dir(common.join("worktrees")) {
        let mut linked: Vec<Worktree> = entries
            .flatten()
            .filter_map(|e| linked_worktree(&e.path()))
            .collect();
        linked.sort_by(|a, b| a.path.cmp(&b.path));
        repo.worktrees.extend(linked);
    }
    Some(repo)
}

fn linked_worktree(entry: &Path) -> Option<Worktree> {
    let gitdir = fs::read_to_string(entry.join("gitdir")).ok()?;
    let dotgit = entry.join(gitdir.trim());
    let path = canonical(dotgit.parent()?);
    Some(Worktree {
        name: dir_name(&path),
        head: read_head(&entry.join("HEAD")),
        is_main: false,
        locked: entry.join("locked").exists(),
        prunable: !dotgit.exists(),
        path,
        status: None,
        disk_bytes: None,
    })
}

fn read_head(file: &Path) -> Head {
    let Ok(s) = fs::read_to_string(file) else {
        return Head::Unknown;
    };
    let s = s.trim();
    if let Some(r) = s.strip_prefix("ref:") {
        let r = r.trim();
        return Head::Branch(r.strip_prefix("refs/heads/").unwrap_or(r).to_string());
    }
    if s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Head::Detached(s[..7].to_string());
    }
    Head::Unknown
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

fn repo_name(home: &Path) -> String {
    let name = dir_name(home);
    match name.strip_suffix(".git") {
        Some(stripped) if !stripped.is_empty() => stripped.to_string(),
        _                                      => name,
    }
}
