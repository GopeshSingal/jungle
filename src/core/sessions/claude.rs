use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{agent_dir, Matcher};
use crate::core::{model::Session, sessions::MAX_PER_WORKTREE};

// how much of the transcript's end to read, looking for its last timestamp
const TAIL_BYTES: u64 = 32 * 1024;

pub fn dir() -> PathBuf {
    agent_dir("CLAUDE_CONFIG_DIR", ".claude")
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default)]
    display: String,
    timestamp: u64,  // milliseconds
    project: PathBuf,
    #[serde(rename = "sessionId")]
    session_id: String,
}

#[derive(Deserialize)]
struct Record {
    timestamp: Option<String>,
}

struct Seen {
    project:   PathBuf,
    last_at:   u64,  // milliseconds
    prompt_at: u64,
    prompt:    Option<String>,
}

pub fn collect(dir: &Path, paths: &[PathBuf]) -> Vec<(usize, Session)> {
    let mut matcher = Matcher::new(paths);
    let mut found: HashMap<usize, Vec<(Session, PathBuf)>> = HashMap::new();
    for (id, seen) in read_history(&dir.join("history.jsonl")) {
        let Some(i) = matcher.find(&seen.project) else {
            continue;
        };
        // claude names the folder after the path it saw it as, not the canonical
        let transcript = dir
            .join("projects")
            .join(encode_project(&seen.project))
            .join(format!("{id}.jsonl"));
        // no transcript means claude cleaned it up -> cannot be resumed
        if !transcript.is_file() {
            continue;
        }
        let session = Session {
            agent:          "claude".into(),
            id,
            summary:        seen.prompt,
            last_active_at: seen.last_at / 1000,
        };
        found.entry(i).or_default().push((session, transcript));
    }

    let mut out = Vec::new();
    for (i, mut found) in found {
        // rank by last prompt then refine survivors
        found.sort_by_key(|f| Reverse(f.0.last_active_at));
        found.truncate(MAX_PER_WORKTREE);
        for (mut s, transcript) in found {
            if let Some(t) = last_record_at(&transcript) {
                s.last_active_at = s.last_active_at.max(t);
            }
            out.push((i, s));
        }
    }
    out
}

// session id -> where it ran, when last prompted, last real prompt
fn read_history(file: &Path) -> HashMap<String, Seen> {
    let mut out: HashMap<String, Seen> = HashMap::new();
    let Ok(f) = File::open(file) else {
        return out;
    };
    for line in BufReader::new(f).lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(e) = serde_json::from_str::<Entry>(&line) else {
            continue;
        };
        let prompt = prompt_text(&e.display);
        let seen = out.entry(e.session_id).or_insert_with(|| Seen {
            project:   e.project.clone(),
            last_at:   0,
            prompt_at: 0,
            prompt:    None,
        });
        if e.timestamp >= seen.last_at {
            seen.last_at = e.timestamp;
            seen.project = e.project;
        }
        if prompt.is_some() && e.timestamp >= seen.prompt_at {
            seen.prompt_at = e.timestamp;
            seen.prompt = prompt;
        }
    }
    out
}

// unix time of the newest timestamped record
fn last_record_at(transcript: &Path) -> Option<u64> {
    let mut f = File::open(transcript).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let mut lines: Vec<&[u8]> = buf.split(|&b| b == b'\n').collect();
    if start > 0 {
        lines.remove(0);
    }
    lines
        .iter()
        .rev()
        .filter_map(|l| serde_json::from_slice::<Record>(l).ok()?.timestamp)
        .find_map(|t| parse_utc(&t))
}

fn parse_utc(s: &str) -> Option<u64> {
    if s.len() < 20 || !s.ends_with('Z') || s.as_bytes()[10] != b'T' {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // see Howard Hinnant's days_from_civil
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec).ok()
}

// claude's folder name for a project
// non-alphanumeric -> `-`
fn encode_project(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

// first non-empty line (ignore bare commands)
fn prompt_text(display: &str) -> Option<String> {
    let line = display.lines().map(str::trim).find(|l| !l.is_empty())?;
    if line.starts_with('/') && !line.contains(char::is_whitespace) {
        return None;
    }
    Some(line.to_string())
}
