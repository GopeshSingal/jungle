use crate::core::model::{Index, Worktree};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum SortKey {
    /// worktree name, A-Z
    Name,
    /// branch name, A-Z (detached heads last)
    Branch,
    /// full worktree path, A-Z
    Path,
    /// newest commit first
    LastCommit,
    /// largest worktree first (measures disk usage)
    Disk,
}

pub(super) fn sort_index(idx: &mut Index, key: SortKey, reverse: bool) {
    use std::cmp::Ordering;

    for repo in &mut idx.repos {
        repo.worktrees.sort_by(|a, b| compare_worktrees(a, b, key, reverse));
    }
    idx.repos.sort_by(|a, b| match (a.worktrees.first(), b.worktrees.first()) {
        (Some(a), Some(b)) => compare_worktrees(a, b, key, reverse),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.common_dir.cmp(&b.common_dir),
    });
}

pub(super) fn compare_worktrees(a: &Worktree, b: &Worktree, key: SortKey, reverse: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    fn optional<T: Ord>(a: Option<T>, b: Option<T>, reverse: bool) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) => if reverse { b.cmp(&a) } else { a.cmp(&b) },
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    let order = match key {
        SortKey::Name => optional(Some(&a.name), Some(&b.name), reverse),
        SortKey::Branch => optional(a.branch(), b.branch(), reverse),
        SortKey::Path => optional(Some(&a.path), Some(&b.path), reverse),
        SortKey::LastCommit => optional(
            a.status.as_ref().and_then(|s| s.last_commit_at),
            b.status.as_ref().and_then(|s| s.last_commit_at),
            !reverse,
        ),
        SortKey::Disk => optional(a.disk_bytes, b.disk_bytes, !reverse),
    };
    order.then_with(|| {
        let tie = a.path.cmp(&b.path);
        if reverse { tie.reverse() } else { tie }
    })
}

