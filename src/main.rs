use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use clap::{Parser, ValueEnum};
use serde::Serialize;

use cruft::delete::{delete_interactive, delete_targets, Decision, DeleteOpts, TrashRemover};
use cruft::filter::{parse_duration, parse_size, Filters};
use cruft::format::{human_age, human_size};
use cruft::scan::{scan, Found, Kind, SizeMode};
use cruft::sort::{sort_found, SortKey};

#[derive(Parser)]
#[command(
    name = "cruft",
    version,
    about = "Find disk space you can reclaim: node_modules, Rust target, Python caches"
)]
struct Cli {
    /// Directory to scan
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Print results as JSON
    #[arg(long)]
    json: bool,

    /// Measure logical file size instead of on-disk usage
    #[arg(long)]
    apparent: bool,

    /// Keep only entries at least this big (e.g. 500K, 1.5G; plain number is bytes)
    #[arg(long, value_parser = parse_size)]
    min_size: Option<u64>,

    /// Keep only entries untouched for at least this long (e.g. 12h, 30d, 2w)
    #[arg(long, value_parser = parse_duration)]
    older_than: Option<Duration>,

    /// Keep only these comma-separated types: node_modules, target, __pycache__, venv, pytest_cache, mypy_cache, ruff_cache, next, nuxt, turbo, svelte-kit, parcel-cache, gradle, tox
    #[arg(long, value_parser = parse_only)]
    only: Option<KindList>,

    /// Sort by size (biggest), modified (oldest), or path
    #[arg(long, value_enum, default_value = "size")]
    sort: SortArg,

    /// Reverse the sort order
    #[arg(short = 'r', long)]
    reverse: bool,

    /// Show only the largest N entries (after sorting)
    #[arg(long)]
    limit: Option<NonZeroUsize>,

    /// Print just the reclaimable total, no table (ignores --limit)
    #[arg(long)]
    total_only: bool,

    /// Move the reclaimable directories to the trash after scanning
    #[arg(long)]
    delete: bool,

    /// With --delete, skip the confirmation prompt
    #[arg(short = 'y', long)]
    yes: bool,

    /// With --delete, ask about each directory (y/n/q)
    #[arg(short = 'i', long)]
    interactive: bool,

    /// With --delete, list what would be trashed without touching anything
    #[arg(long)]
    dry_run: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.delete && cli.json {
        eprintln!("cruft: --json is not supported with --delete");
        return ExitCode::FAILURE;
    }
    if cli.total_only && cli.delete {
        eprintln!("cruft: --total-only cannot be combined with --delete");
        return ExitCode::FAILURE;
    }
    if (cli.yes || cli.dry_run || cli.interactive) && !cli.delete {
        eprintln!("cruft: --yes, --interactive, and --dry-run only apply with --delete");
        return ExitCode::FAILURE;
    }
    if cli.yes && cli.interactive {
        eprintln!("cruft: -y and --interactive are mutually exclusive");
        return ExitCode::FAILURE;
    }

    if !cli.path.is_dir() {
        eprintln!("cruft: {}: not a directory", cli.path.display());
        return ExitCode::FAILURE;
    }

    let mode = if cli.apparent {
        SizeMode::Apparent
    } else {
        SizeMode::Disk
    };
    let filters = Filters {
        min_size: cli.min_size,
        older_than: cli.older_than,
        kinds: cli.only.map(|k| k.0),
    };

    let filtered = filters.apply(scan(&cli.path, mode), SystemTime::now());

    // total-only reports the full filtered sum and ignores --limit
    if cli.total_only {
        let total: u64 = filtered.iter().map(|f| f.size).sum();
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&TotalReport { total_bytes: total }).unwrap()
            );
        } else {
            println!("Reclaimable total: {}", human_size(total));
        }
        return ExitCode::SUCCESS;
    }

    // filter -> sort -> limit gives one final list; table, JSON, and the delete
    // set all read from it, so `cruft --delete --limit N` trashes exactly the N shown.
    let mut found = filtered;
    sort_found(&mut found, cli.sort.into(), cli.reverse);
    if let Some(n) = cli.limit {
        found.truncate(n.get());
    }

    if found.is_empty() {
        if cli.json {
            print_json(&found);
        } else if filters.active() {
            println!("No matching directories.");
        } else {
            println!("Nothing reclaimable found.");
        }
        return ExitCode::SUCCESS;
    }

    if cli.delete {
        run_delete(&found, cli.dry_run, cli.yes, cli.interactive)
    } else if cli.json {
        print_json(&found);
        ExitCode::SUCCESS
    } else {
        print_table(&found);
        ExitCode::SUCCESS
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum SortArg {
    Size,
    Modified,
    Path,
}

impl From<SortArg> for SortKey {
    fn from(s: SortArg) -> Self {
        match s {
            SortArg::Size => SortKey::Size,
            SortArg::Modified => SortKey::Modified,
            SortArg::Path => SortKey::Path,
        }
    }
}

#[derive(Clone)]
struct KindList(Vec<Kind>);

fn parse_only(s: &str) -> Result<KindList, String> {
    cruft::filter::parse_kinds(s).map(KindList)
}

fn run_delete(found: &[Found], dry_run: bool, yes: bool, interactive: bool) -> ExitCode {
    print_table(found);

    let total: u64 = found.iter().map(|f| f.size).sum();

    if dry_run {
        println!(
            "\nWould move {} to trash:",
            summary_count(found.len(), total)
        );
        for f in found {
            println!("  {}", f.rel.display());
        }
        return ExitCode::SUCCESS;
    }

    println!();
    let outcome = if interactive {
        delete_interactive(found, prompt_item, &TrashRemover)
    } else {
        delete_targets(
            found,
            DeleteOpts { dry_run, yes },
            || prompt_confirm(found.len(), total),
            &TrashRemover,
        )
    };

    if outcome.aborted {
        println!("Aborted, nothing deleted.");
        return ExitCode::SUCCESS;
    }

    // anything that moved/disappeared between the scan and now
    for path in &outcome.changed {
        eprintln!("skipped (changed since scan): {}", path.display());
    }

    if interactive {
        println!(
            "\nMoved {} {} to trash, freed ~{}, skipped {}.",
            outcome.moved,
            noun(outcome.moved),
            human_size(outcome.freed),
            outcome.skipped
        );
    } else {
        println!(
            "\nMoved {} {} to trash, freed ~{}.",
            outcome.moved,
            noun(outcome.moved),
            human_size(outcome.freed)
        );
    }

    if !outcome.failures.is_empty() {
        eprintln!("Failed to move:");
        for (path, err) in &outcome.failures {
            eprintln!("  {}: {err}", path.display());
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn prompt_item(f: &Found) -> Decision {
    print!(
        "{} ({}, {}) trash? [y/N/q] ",
        f.rel.display(),
        f.kind.label(),
        human_size(f.size)
    );
    if io::stdout().flush().is_err() {
        return Decision::Quit;
    }
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => Decision::Quit, // EOF or error: stop rather than loop
        Ok(_) => match line.trim() {
            "y" | "Y" => Decision::Trash,
            "q" | "Q" => Decision::Quit,
            _ => Decision::Skip,
        },
    }
}

fn noun(n: usize) -> &'static str {
    if n == 1 {
        "directory"
    } else {
        "directories"
    }
}

fn summary_count(n: usize, bytes: u64) -> String {
    format!("{n} {} ({})", noun(n), human_size(bytes))
}

fn prompt_confirm(n: usize, total: u64) -> bool {
    print!("\nMove {} to trash? [y/N] ", summary_count(n, total));
    if io::stdout().flush().is_err() {
        return false;
    }
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y")
}

fn age_string(modified: Option<SystemTime>) -> String {
    match modified.and_then(|m| SystemTime::now().duration_since(m).ok()) {
        Some(d) => human_age(d),
        None => "-".into(),
    }
}

fn print_table(found: &[Found]) {
    let mut rows: Vec<[String; 4]> = Vec::with_capacity(found.len());
    for f in found {
        rows.push([
            f.rel.display().to_string(),
            f.kind.label().to_string(),
            human_size(f.size),
            age_string(f.modified),
        ]);
    }

    let headers = ["PATH", "TYPE", "SIZE", "MODIFIED"];
    let mut width = [0usize; 4];
    for (i, h) in headers.iter().enumerate() {
        width[i] = h.len();
    }
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            width[i] = width[i].max(cell.len());
        }
    }

    // SIZE reads better right-aligned; the rest left-aligned
    println!(
        "{:<pw$}  {:<tw$}  {:>sw$}  {}",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        pw = width[0],
        tw = width[1],
        sw = width[2],
    );
    for row in &rows {
        println!(
            "{:<pw$}  {:<tw$}  {:>sw$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            pw = width[0],
            tw = width[1],
            sw = width[2],
        );
    }

    let total: u64 = found.iter().map(|f| f.size).sum();
    println!("\nReclaimable total: {}", human_size(total));
}

#[derive(Serialize)]
struct JsonEntry<'a> {
    path: String,
    kind: &'a str,
    size_bytes: u64,
    modified_unix: Option<u64>,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    entries: Vec<JsonEntry<'a>>,
    total_bytes: u64,
}

#[derive(Serialize)]
struct TotalReport {
    total_bytes: u64,
}

fn print_json(found: &[Found]) {
    let entries: Vec<JsonEntry> = found
        .iter()
        .map(|f| JsonEntry {
            path: f.rel.display().to_string(),
            kind: f.kind.label(),
            size_bytes: f.size,
            modified_unix: f
                .modified
                .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        })
        .collect();

    let report = JsonReport {
        total_bytes: found.iter().map(|f| f.size).sum(),
        entries,
    };
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
