pub mod output;
pub mod shell;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use crate::core::aliases::{self, Outcome};
use crate::core::config::{self, Config};
use crate::core::model::Index;
use crate::core::{index, new, rm};

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
    /// create a linked worktree under <repo>/.jungle/
    New {
        /// `[repo] <branch>` — repo defaults to the repo containing cwd
        #[arg(required = true, num_args = 1..=2)]
        targets: Vec<String>,
        /// path for the new worktree (default: <repo>/.jungle/<branch>)
        #[arg(long)]
        path: Option<PathBuf>,
        /// start point when creating a new branch
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
    /// list every alias and its path
    Aliases {
        /// names only
        #[arg(long)]
        names: bool,
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
            merged,
            risk,
        } => {
            warn_if_no_roots(&cfg);
            let force = rescan || scan.disk || scan.untracked;
            let idx = index::load_or_scan(&cfg, force, scan.disk, scan.untracked)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&idx)?);
            } else {
                output::table(&idx, &output::Filter { merged, risk });
            }
        }
        Command::Risk { rescan } => {
            let idx = index::load_or_scan(&cfg, rescan, false, false)?;
            output::risk(&idx);
        }
        Command::Prune { rescan, apply } => {
            let idx = index::load_or_scan(&cfg, rescan, false, false)?;
            if apply {
                return prune_apply(&cfg, &idx);
            }
            output::prune(&idx);
        }
        Command::Resolve { query } => return resolve(&cfg, &query.join(" ")),
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
        Command::Aliases { names } => {
            let idx = index::load_fresh(&cfg, FRESH_SECS);
            for (alias, path) in aliases::all(&idx, &cfg) {
                if names {
                    println!("{alias}");
                } else {
                    println!("{alias}\t{}", path.display());
                }
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
    let removed = rm::prune_apply(idx)?;
    if removed.is_empty() {
        println!("Nothing merged and clean to remove.");
    } else {
        for path in &removed {
            println!("{}", path.display());
        }
        println!("Removed {} worktree(s).", removed.len());
    }
    let idx = index::rediscover(cfg, Some(idx));
    index::save(&idx)?;
    Ok(ExitCode::SUCCESS)
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
            "jungle: none of the configured roots exist. Set `roots` in {}",
            output::tilde(&config::config_path())
        );
    }
}
