//! `jevision` — checks and formats JEVision test files.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use jevision_core::{Issue, Severity};
use serde::Serialize;

const EXIT_HELP: &str = "Exit status: 0 — all good, 1 — problems found, 2 — wrong arguments or a file could not be read or written.";

#[derive(Parser)]
#[command(name = "jevision", version, about = "Checks and formats JEVision test files", after_help = EXIT_HELP)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check test files for errors and warnings.
    #[command(after_help = EXIT_HELP)]
    Validate {
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
        /// Test files, or folders to search for `*.flow.json` files.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Rewrite test files in canonical form.
    #[command(after_help = EXIT_HELP)]
    Fmt {
        /// Only report files that are not in canonical form; change nothing.
        #[arg(long)]
        check: bool,
        /// Test files, or folders to search for `*.flow.json` files.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

/// Outcome of a whole command, ordered by severity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Ok = 0,
    Problems = 1,
    IoFailure = 2,
}

fn main() -> ExitCode {
    let status = match Cli::parse().command {
        Command::Validate { json, paths } => validate(&paths, json),
        Command::Fmt { check, paths } => fmt(&paths, check),
    };
    ExitCode::from(status as u8)
}

#[derive(Serialize)]
struct FileReport {
    file: String,
    valid: bool,
    issues: Vec<Issue>,
}

fn validate(paths: &[PathBuf], json: bool) -> Status {
    let (files, mut status) = collect(paths);
    let mut reports = Vec::new();
    for file in files {
        let text = match fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                status = status.max(io_failure(&file, "read", &e));
                continue;
            }
        };
        let issues = match jevision_core::parse(&text) {
            Ok(flow) => jevision_core::validate(&flow),
            Err(e) => e.issues,
        };
        let valid = !issues.iter().any(|i| i.severity == Severity::Error);
        if !valid {
            status = status.max(Status::Problems);
        }
        reports.push(FileReport {
            file: file.display().to_string(),
            valid,
            issues,
        });
    }
    if json {
        let text = serde_json::to_string_pretty(&reports).expect("reports serialize to JSON");
        println!("{text}");
    } else {
        for r in &reports {
            print_report(r);
        }
    }
    status
}

fn print_report(r: &FileReport) {
    if r.issues.is_empty() {
        println!("{}: ok", r.file);
        return;
    }
    let count = |s| r.issues.iter().filter(|i| i.severity == s).count();
    let (errors, warnings) = (count(Severity::Error), count(Severity::Warning));
    println!(
        "{}: {errors} error{}, {warnings} warning{}",
        r.file,
        plural(errors),
        plural(warnings)
    );
    for issue in &r.issues {
        let level = match issue.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let params: Vec<String> = issue
            .params
            .iter()
            .map(|p| {
                let plain = !p.value.is_empty() && !p.value.contains(char::is_whitespace);
                if plain {
                    format!("{}={}", p.name, p.value)
                } else {
                    format!("{}={:?}", p.name, p.value)
                }
            })
            .collect();
        println!("  {level:<7} {} {}", issue.code, params.join(" "));
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn fmt(paths: &[PathBuf], check: bool) -> Status {
    let (files, mut status) = collect(paths);
    for file in files {
        let text = match fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => {
                status = status.max(io_failure(&file, "read", &e));
                continue;
            }
        };
        let canonical = match jevision_core::canonicalize(&text) {
            Ok(c) => c,
            Err(e) => {
                print_report(&FileReport {
                    file: file.display().to_string(),
                    valid: false,
                    issues: e.issues,
                });
                status = status.max(Status::Problems);
                continue;
            }
        };
        if canonical == text {
            continue;
        }
        if check {
            println!("{}: not formatted", file.display());
            status = status.max(Status::Problems);
        } else if let Err(e) = write_atomically(&file, &canonical) {
            status = status.max(io_failure(&file, "write", &e));
        } else {
            println!("{}: formatted", file.display());
        }
    }
    status
}

/// Writes through a temporary file in the same folder, so a crash never leaves half a test.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&tmp, text)?;
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(&tmp, meta.permissions())?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn io_failure(path: &Path, action: &str, e: &io::Error) -> Status {
    eprintln!("{}: cannot {action}: {e}", path.display());
    Status::IoFailure
}

/// Expands folders into the `*.flow.json` files inside them, sorted by path.
fn collect(paths: &[PathBuf]) -> (Vec<PathBuf>, Status) {
    let mut files = Vec::new();
    let mut status = Status::Ok;
    for path in paths {
        if path.is_dir() {
            if let Err(e) = walk(path, &mut files) {
                status = status.max(io_failure(path, "read", &e));
            }
        } else {
            files.push(path.clone());
        }
    }
    (files, status)
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<io::Result<_>>()?;
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(&path, files)?;
        } else if name.ends_with(".flow.json") {
            files.push(path);
        }
    }
    Ok(())
}
