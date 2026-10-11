use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub roots:     Vec<String>,
    pub max_depth: usize,
    pub ignore:    Vec<String>,
    pub aliases:   BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: ["~/code", "~/work", "~/src", "~/projects"]
                .into_iter()
                .map(String::from)
                .collect(),
            max_depth: 3,
            ignore: ["node_modules", "target", ".venv", "venv", "vendor", "dist", "build"]
                .into_iter()
                .map(String::from)
                .collect(),
            aliases: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parsing {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }
    
    pub fn root_paths(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|r| expand(r)).filter(|p| p.is_dir()).collect()
    }

    pub fn alias_paths(&self) -> Vec<(String, PathBuf)> {
        self.aliases
            .iter()
            .map(|(name, path)| (name.clone(), canonical(&expand(path))))
            .collect()
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _                        => home().join(fallback)
    }
}

pub fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("jungle").join("config.toml")
}

pub fn cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join("jungle")
}

pub fn expand(p: &str) -> PathBuf {
    if p == "~" {
        home()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(p)
    }
}

pub fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}
