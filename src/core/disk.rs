use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub fn usage(path: &Path) -> u64 {
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        }
        for entry in entries.flatten() {
            let Ok(md) = entry.metadata() else {
                continue;
            }
            if md.is_dir() {
                if entry.file_name() != ".git" {
                    stack.push(entry.path());
                }
            } else if md.is_file() {
                total += file_size(&md, &mut seen);
            }
        }
    }
    total
}

#[cfg(unix)]
fn file_size(md: &fs::Metadata, seen: &mut HashSet<(u64, u64)>) -> u64 {
    use std::os::unix::fs::MetadataExt;
    if md.nlink() > 1 && !seen.insert((md.dev(), md.ino())) {
        return 0;
    }
    md.blocks() * 512
}

#[cfg(not(unix))]
fn file_size(md: &fs::Metadata, _seen: &mut HashSet<(u64, u64)>) -> u64 {
    md.len()
}
