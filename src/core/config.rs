use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub editor:    Option<String>,
    pub roots:     Vec<String>,
    pub max_depth: usize,
    pub ignore:    Vec<String>,
    pub aliases:   BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: None,
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

pub fn set_alias(name: &str, target: &Path) -> Result<PathBuf> {
    set_alias_in(&config_path(), name, target)
}

fn set_alias_in(path: &Path, name: &str, target: &Path) -> Result<PathBuf> {
    if name.is_empty() || name.trim() != name || name.chars().any(char::is_control) || name == "-" {
        bail!("alias must be nonempty, have no surrounding whitespace or control characters, and not be '-'");
    }
    let target = std::fs::canonicalize(expand(&target.to_string_lossy()))
        .with_context(|| format!("resolving {}", target.display()))?;
    if !target.is_dir() {
        bail!("alias target must be a directory: {}", target.display());
    }
    let target_text = target.to_str().context("alias target is not valid UTF-8")?;
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut doc = source.parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    if !doc.contains_key("aliases") {
        doc["aliases"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let aliases = doc["aliases"].as_table_like_mut().context("aliases must be a TOML table")?;
    aliases.insert(name, toml_edit::value(target_text));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating config directory")?;
    }
    std::fs::write(path, doc.to_string()).with_context(|| format!("writing {}", path.display()))?;
    Ok(target)
}

pub fn remove_alias(name: &str) -> Result<()> {
    remove_alias_in(&config_path(), name)
}

fn remove_alias_in(path: &Path, name: &str) -> Result<()> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!("no custom alias named {name:?}; generated aliases cannot be removed");
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mut doc = source.parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("parsing {}", path.display()))?;
    let removed = match doc.get_mut("aliases") {
        Some(item) => item.as_table_like_mut()
            .context("aliases must be a TOML table")?
            .remove(name).is_some(),
        None => false,
    };
    if !removed {
        bail!("no custom alias named {name:?}; generated aliases cannot be removed");
    }
    std::fs::write(path, doc.to_string()).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
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
