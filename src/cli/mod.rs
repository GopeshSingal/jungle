pub mod output;
pub mod shell;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};

use crate::core::aliases::{self, Outcome};
use crate::core::config::{self, Config};
use crate::core::model::{Index, Session};
use crate::core::{git, index, new, notes, rm, sessions};

const FRESH_SECS: u64 = 30;

#[derive(Parser)]
#[command(
    name = "jungle",
    version,
    about = "track git worktrees on your machine"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// rediscover repos and refresh status on every worktree
    Scan(ScanArgs),
    /// show every worktree across repos
    Ls {
        #[command(flatten)]
        scan: ScanArgs,
        /// rescan before listing
        #[arg(long)]
        rescan: bool,
        /// print the index as JSON
        #[arg(long)]
        json: bool,
        /// only this repository (use parent/name to distinguish duplicate names)
        #[arg(long, value_name = "NAME")]
        repo: Option<String>,
        /// only merged worktrees
        #[arg(long)]
        merged: bool,
        /// only worktrees with risky work
        #[arg(long)]
        risk: bool,
    },
    /// list work that only exists on this machine
    Risk {
        /// rescan before listing
        #[arg(long)]
        rescan: bool,
    },
    /// print or apply removals of merged, clean worktrees
    Prune {
        /// rescan before listing
        #[arg(long)]
        rescan: bool,
        /// actually remove instead of printing commands
        #[arg(long)]
        apply: bool,
    },
    /// print the path for an alias
    Resolve {
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
    },
    /// open a worktree in the configured editor (or VISUAL / EDITOR)
    Open {
        /// alias (defaults to `.` for the current worktree)
        #[arg(default_value = ".", num_args = 1..)]
        query: Vec<String>,
    },
    /// create a linked worktree under <repo>/.jungle/
    New {
        /// `[repo] <branch>` — repo defaults to the repo containing cwd
        #[arg(required = true, num_args = 1..=2)]
        targets: Vec<String>,
        /// path for the new worktree (default: <repo>/.jungle/<branch>)
        #[arg(long)]
        path: Option<PathBuf>,
        /// start point when creating a new branch (default: origin/<branch> if it exists, else the default branch)
        #[arg(long)]
        base: Option<String>,
    },
    /// remove a linked worktree by alias
    Rm {
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// remove even if dirty, unmerged, or locked
        #[arg(long)]
        force: bool,
        /// rescan before resolving
        #[arg(long)]
        rescan: bool,
    },
    /// create or update a custom alias in the config
    Alias {
        /// shortcut name
        name: String,
        /// target directory (defaults to the current directory)
        path: Option<PathBuf>,
    },
    /// remove a custom alias from the config
    Unalias {
        /// custom alias name
        name: String,
    },
    /// list every alias and its path
    Aliases {
        /// names only
        #[arg(long)]
        names: bool,
    },
    /// save or edit a local note for the current worktree
    #[command(long_about = "Save or edit a local note for the current worktree.\n\nWith no options, open the existing note in Git's configured editor.\nEmpty input leaves the existing note unchanged; use --clear to remove it.")]
    Note {
        /// replace the note without opening an editor
        #[arg(short, long, conflicts_with_all = ["show", "clear"])]
        message: Option<String>,
        /// print the full note
        #[arg(long, conflicts_with = "clear")]
        show: bool,
        /// remove the note
        #[arg(long)]
        clear: bool,
    },
    /// list agent sessions per worktree
    Sessions {
        /// only the worktree this alias points at, or `.` for the current one
        query: Vec<String>,
    },
    /// resume the newest agent session in a worktree
    Resume {
        /// alias (defaults to `.` for the current worktree)
        #[arg(default_value = ".", num_args = 1..)]
        query: Vec<String>,
        /// print the command instead of running it
        #[arg(long)]
        print: bool,
        /// resume the nth newest session
        #[arg(long, default_value_t = 1)]
        pick: usize,
        /// only consider this agent's sessions
        #[arg(long, value_parser = ["claude", "codex"])]
        agent: Option<String>,
    },
    /// show the config file, roots, and aliases in use
    Config {
        /// print only the config file path
        #[arg(long, conflicts_with = "json")]
        path: bool,
        /// print the settings as JSON
        #[arg(long)]
        json: bool,
    },
    /// print shell integration; add `eval "$(jungle init zsh)"` to shell config
    Init {
        shell: shell::Shell,
        /// name of the jump command
        #[arg(long, default_value = "j")]
        cmd: String,
    },
}

#[derive(Args, Clone, Copy)]
struct ScanArgs {
    // measure disk usage per worktree (slow on large worktrees)
    #[arg(long)]
    disk: bool,
    // check for untracked files
    #[arg(long)]
    untracked: bool,
}

pub fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    // before loading, so a config that fails to parse can still be found
    if let Command::Config { path: true, .. } = cli.command {
        println!("{}", config::config_path().display());
        return Ok(ExitCode::SUCCESS);
    }
    let cfg = Config::load()?;
    match cli.command {
        Command::Scan(args) => {
            warn_if_no_roots(&cfg);
            let started = Instant::now();
            let idx = index::full_scan(&cfg, args.disk, args.untracked);
            index::save(&idx)?;
            println!("{}", output::summary(&idx));
            println!(
                "Scanned in {:.1}s. Index saved to {}",
                started.elapsed().as_secs_f64(),
                output::tilde(&index::path())
            );
        }
        Command::Ls {
            scan,
            rescan,
            json,
            repo,
            merged,
            risk,
        } => {
            warn_if_no_roots(&cfg);
            let force = rescan || scan.disk || scan.untracked;
            let mut idx = index::load_or_scan(&cfg, force, scan.disk, scan.untracked)?;
            notes::attach(&mut idx)?;
            let filter = output::Filter { repo, merged, risk };
            if json {
                println!("{}", serde_json::to_string_pretty(&output::filtered_index(&idx, &filter))?);
            } else {
                output::table(&idx, &filter, &cfg);
            }
        }
        Command::Risk { rescan } => {
            let idx = index::load_or_scan(&cfg, rescan, false, false)?;
            output::risk(&idx, &cfg);
        }
        Command::Prune { rescan, apply } => {
            let mut idx = index::load_or_scan(&cfg, rescan, false, false)?;
            index::check_untracked(&mut idx);
            if apply {
                return prune_apply(&cfg, &idx);
            }
            index::save(&idx)?;
            output::prune(&idx);
        }
        Command::Resolve { query } => return resolve(&cfg, &query.join(" ")),
        Command::Open { query } => {
            let idx = index::load_fresh(&cfg, FRESH_SECS);
            let path = target(&idx, &cfg, &query.join(" "))?;
            crate::core::open::open(&cfg, &path)?;
        }
        Command::New {
            targets,
            path,
            base,
        } => return new_worktree(&cfg, targets, path, base),
        Command::Rm {
            query,
            force,
            rescan,
        } => return rm_worktree(&cfg, &query.join(" "), force, rescan),
        Command::Alias { name, path } => {
            let path = path.unwrap_or(std::env::current_dir().context("reading current directory")?);
            let path = config::set_alias(&name, &path)?;
            println!("{name}\t{}", path.display());
        }
        Command::Unalias { name } => {
            config::remove_alias(&name)?;
            println!("removed alias {name}");
        }
        Command::Aliases { names } => {
            let idx = index::load_fresh(&cfg, FRESH_SECS);
            let entries = aliases::all(&idx, &cfg);
            if names {
                for (alias, _) in entries {
                    println!("{alias}");
                }
            } else {
                output::aliases(&entries, &cfg);
            }
        }
        Command::Note { message, show, clear } => {
            let cwd = std::env::current_dir().context("reading current directory")?;
            let path = notes::path(&cwd)?;
            if show {
                if let Some(note) = notes::read(&path)? {
                    println!("{note}");
                }
            } else if clear {
                notes::clear(&path)?;
            } else if let Some(message) = message {
                notes::save(&path, &message)?;
            } else {
                notes::edit(&cwd, &path)?;
            }
        }
        Command::Sessions { query } => {
            let idx = with_sessions(&cfg);
            let only = match query.is_empty() {
                true  => None,
                false => Some(target(&idx, &cfg, &query.join(" "))?),
            };
            output::sessions(&idx, only.as_deref(), &cfg);
        }
        Command::Resume {
            query,
            print,
            pick,
            agent,
        } => return resume(&cfg, &query.join(" "), print, pick, agent.as_deref()),
        Command::Config { json, .. } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&output::config_json(&cfg))?);
            } else {
                output::config(&cfg);
            }
        }
        Command::Init { shell: sh, cmd } => println!("{}", shell::init(sh, &cmd)?),
    }
    Ok(ExitCode::SUCCESS)
}

fn new_worktree(
    cfg: &Config,
    targets: Vec<String>,
    path: Option<PathBuf>,
    base: Option<String>,
) -> Result<ExitCode> {
    let (repo, branch) = match targets.len() {
        1 => (None, targets[0].clone()),
        2 => (Some(targets[0].clone()), targets[1].clone()),
        _ => unreachable!("clap enforces 1..=2 args"),
    };
    let idx = index::load_fresh(cfg, FRESH_SECS);
    let created = new::create(
        cfg,
        &idx,
        new::Args {
            repo,
            branch,
            path,
            base,
        },
    )?;
    let idx = index::rediscover(cfg, Some(&idx));
    index::save(&idx)?;
    println!("{}", created.display());
    Ok(ExitCode::SUCCESS)
}

fn rm_worktree(cfg: &Config, query: &str, force: bool, rescan: bool) -> Result<ExitCode> {
    let idx = index::load_or_scan(cfg, rescan, false, false)?;
    let removed = rm::remove(cfg, &idx, query, force)?;
    let idx = index::rediscover(cfg, Some(&idx));
    index::save(&idx)?;
    println!("{}", removed.display());
    Ok(ExitCode::SUCCESS)
}

fn prune_apply(cfg: &Config, idx: &Index) -> Result<ExitCode> {
    let pruned = rm::prune_apply(idx);
    // save before reporting, so aliases stop pointing at removed worktrees even if some failed
    let idx = index::rediscover(cfg, Some(idx));
    index::save(&idx)?;

    if pruned.removed.is_empty() && pruned.failed.is_empty() {
        println!("Nothing merged and clean to remove.");
    } else if !pruned.removed.is_empty() {
        for path in &pruned.removed {
            println!("{}", path.display());
        }
        println!("Removed {} worktree(s).", pruned.removed.len());
    }
    for (home, err) in &pruned.prune_failed {
        eprintln!("jungle: couldn't prune stale worktrees of {}: {err}", output::tilde(home));
    }
    if !pruned.failed.is_empty() {
        for (path, err) in &pruned.failed {
            eprintln!("jungle: skipped {}: {err}", output::tilde(path));
        }
        eprintln!("Skipped {} worktree(s). Inspect them, then use `jungle rm --force` if you're sure.", pruned.failed.len());
    }
    let ok = pruned.failed.is_empty() && pruned.prune_failed.is_empty();
    Ok(if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn with_sessions(cfg: &Config) -> Index {
    let mut idx = index::load_fresh(cfg, FRESH_SECS);
    sessions::attach(&mut idx.repos);
    idx
}

// an alias, or `.` for the worktree containing cwd
fn target(idx: &Index, cfg: &Config, query: &str) -> Result<PathBuf> {
    if query.trim() != "." {
        return aliases::resolve_one(idx, cfg, query);
    }
    let cwd = std::env::current_dir().context("reading current directory")?;
    let top = git::toplevel(&cwd).context("not inside a git worktree")?;
    let path = config::canonical(&top);
    if idx.worktree_at(&path).is_none() {
        bail!("{} isn't a known worktree; run `jungle scan`", output::tilde(&path));
    }
    Ok(path)
}

fn resume(cfg: &Config, query: &str, print: bool, pick: usize, agent: Option<&str>) -> Result<ExitCode> {
    let idx = with_sessions(cfg);
    let path = target(&idx, cfg, query)?;
    let (_, wt) = idx
        .worktree_at(&path)
        .with_context(|| format!("no worktree at {}", path.display()))?;
    let candidates: Vec<&Session> = wt
        .sessions
        .iter()
        .filter(|s| agent.is_none_or(|a| s.agent == a))
        .collect();
    let Some(s) = candidates.get(pick.saturating_sub(1)) else {
        let whose = agent.map(|a| format!("{a} ")).unwrap_or_default();
        if candidates.is_empty() {
            bail!("no {whose}sessions found in {}", output::tilde(&wt.path));
        }
        bail!("{} has only {} recent {whose}sessions", output::tilde(&wt.path), candidates.len());
    };
    let Some((program, args)) = sessions::resume_command(s) else {
        bail!("don't know how to resume {} sessions", s.agent);
    };

    if print {
        let quoted: Vec<String> = args.iter().map(|a| output::shell_word(a)).collect();
        println!("cd {} && {program} {}", output::shell_quote(&wt.path.to_string_lossy()), quoted.join(" "));
        return Ok(ExitCode::SUCCESS);
    }
    // agents look sessions up by, or resume into, the working dir 
    let mut cmd = std::process::Command::new(&program);
    cmd.args(&args).current_dir(&wt.path);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        bail!("running {program}: {err}");
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().with_context(|| format!("running {program}"))?;
        Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
    }
}

fn resolve(cfg: &Config, query: &str) -> Result<ExitCode> {
    let mut idx = index::load_fresh(cfg, FRESH_SECS);
    let mut outcome = aliases::resolve(&idx, cfg, query);
    if matches!(outcome, Outcome::NotFound) {
        // rediscover in case it was created since last discover
        idx = index::rediscover(cfg, Some(&idx));
        let _ = index::save(&idx);
        outcome = aliases::resolve(&idx, cfg, query);
    }
    match outcome {
        Outcome::Found(path) => {
            println!("{}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Outcome::Ambiguous(candidates) => {
            eprintln!("jungle: '{query}' matches more than one place:");
            for (alias, path) in candidates {
                eprintln!("  {alias:<32} {}", output::tilde(&path));
            }
            Ok(ExitCode::from(2))
        }
        Outcome::NotFound => {
            eprintln!("jungle: nothing matches '{query}'");
            Ok(ExitCode::from(1))
        }
    }
}

fn warn_if_no_roots(cfg: &Config) {
    if cfg.root_paths().is_empty() && cfg.aliases.is_empty() {
        eprintln!(
            "jungle: none of the configured roots exist. Run `jungle config` to check them, then set `roots` in {}",
            output::tilde(&config::config_path())
        );
    }
}
