use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::{agent_dir, Matcher};
use crate::core::model::Session;

pub fn dir() -> PathBuf {
    agent_dir("CODEX_HOME", ".codex")
}

struct Thread {
    id:         String,
    cwd:        PathBuf,
    title:      String,
    updated_at: u64,  // seconds
}

pub fn collect(dir: &Path, paths: &[PathBuf]) -> Vec<(usize, Session)> {
    let Some(db) = newest_state_db(dir) else {
        return Vec::new();
    };
    let Ok(threads) = read_threads(&db) else {
        return Vec::new();
    };
    let mut matcher = Matcher::new(paths);
    threads
        .into_iter()
        .filter_map(|t| {
            let i = matcher.find(&t.cwd)?;
            let title = t.title.trim();
            let session = Session {
                agent:          "codex".into(),
                summary:        (!title.is_empty()).then(|| title.to_string()),
                id:             t.id,
                last_active_at: t.updated_at,
            };
            Some((i, session))
        })
        .collect()
}

fn newest_state_db(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let n: u32 = name.strip_prefix("state_")?.strip_suffix(".sqlite")?.parse().ok()?;
            Some((n, e.path()))
        })
        .max_by_key(|(n, _)| *n)
        .map(|(_, p)| p)
}

// unarchived threads minus subagents
fn read_threads(db: &Path) -> rusqlite::Result<Vec<Thread>> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    )?;
    conn.busy_timeout(Duration::from_millis(200))?;
    const COLUMNS: &str = "SELECT id, cwd, title, updated_at FROM threads WHERE archived = 0";
    let with_spawns = format!("{COLUMNS} AND id NOT IN (SELECT child_thread_id FROM thread_spawn_edges)");
    let mut stmt = match conn.prepare(&with_spawns) {
        Ok(stmt) => stmt,
        Err(_)   => conn.prepare(COLUMNS)?, // older or newer schema without spawn edges
    };
    let rows = stmt.query_map([], |r| {
        Ok(Thread {
            id:         r.get(0)?,
            cwd:        PathBuf::from(r.get::<_, String>(1)?),
            title:      r.get(2)?,
            updated_at: r.get::<_, i64>(3)?.max(0) as u64,
        })
    })?;
    rows.collect()
}
