use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use clap::Parser;
use serde::Serialize;

use reclaim::format::{human_age, human_size};
use reclaim::scan::{scan, Found};

#[derive(Parser)]
#[command(
    name = "reclaim",
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if !cli.path.is_dir() {
        eprintln!("reclaim: {}: not a directory", cli.path.display());
        return ExitCode::FAILURE;
    }

    let mut found = scan(&cli.path);
    found.sort_by(|a, b| b.size.cmp(&a.size));

    if cli.json {
        print_json(&found);
    } else {
        print_table(&found);
    }
    ExitCode::SUCCESS
}

fn age_string(modified: Option<SystemTime>) -> String {
    match modified.and_then(|m| SystemTime::now().duration_since(m).ok()) {
        Some(d) => human_age(d),
        None => "-".into(),
    }
}

fn print_table(found: &[Found]) {
    if found.is_empty() {
        println!("Nothing reclaimable found.");
        return;
    }

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
