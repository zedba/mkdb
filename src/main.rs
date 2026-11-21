//! List targets, variables, and where they are defined, from a GNU make database.
//! See README.md for usage and output format.

use clap::Parser;
use std::io::{self, BufWriter, Read, Write};
use std::path::PathBuf;

/// List targets, variables, and where they are defined, from a GNU make database.
///
/// Reads the output of `make -pRrq` from DUMP files, or from stdin if none are
/// given. By default, lists phony targets and variables.
#[derive(Parser)]
struct Args {
    /// Also list file targets and pattern rules
    #[arg(short = 'a')]
    all: bool,
    /// Make database dumps to read
    dump: Vec<PathBuf>,
}

const ORIGINS: &[&str] = &[
    "default",
    "environment",
    "environment under -e",
    "makefile",
    "command line",
    "'override' directive",
    "automatic",
];

/// Maps a section header to the kind of its entries; None ends parsing.
fn section_kind(line: &str) -> Option<Option<&'static str>> {
    Some(match line {
        "# Variables" => Some("variable"),
        "# Pattern-specific Variable Values" => Some("pattern-var"),
        "# Implicit Rules" => Some("pattern"),
        "# Files" => Some("target"),
        "# Directories" | "# VPATH Search Paths" => None,
        _ => return None,
    })
}

/// Parses `'FILE', line N` into `FILE:N`.
fn location(s: &str) -> Option<String> {
    let (file, n) = s.strip_prefix('\'')?.rsplit_once("', line ")?;
    Some(format!("{file}:{n}"))
}

/// Parses `# ORIGIN` or `# ORIGIN (from 'FILE', line N)`.
fn origin(line: &str) -> Option<String> {
    let rest = line.strip_prefix("# ")?;
    let (name, from) = match rest.split_once(" (from ") {
        Some((name, from)) => (name, Some(from)),
        None => (rest, None),
    };
    if !ORIGINS.contains(&name) {
        return None;
    }
    match from {
        Some(from) => location(from.strip_suffix(')')?),
        None => Some(format!(
            "<{}>",
            name.replace("'override' directive", "override")
        )),
    }
}

fn recipe(line: &str) -> Option<String> {
    location(
        line.strip_prefix("#  recipe to execute (from ")?
            .strip_suffix("):")?,
    )
}

fn varname(text: &str) -> &str {
    let mut words = text.split_whitespace();
    let first = words.next().unwrap_or("");
    if first == "define" {
        words.next().unwrap_or("")
    } else {
        first
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let mut input = Vec::new();
    if args.dump.is_empty() {
        io::stdin().read_to_end(&mut input)?;
    }
    for f in &args.dump {
        input.extend(std::fs::read(f)?);
    }
    let input = String::from_utf8_lossy(&input);

    let mut rows: Vec<(&str, String, String)> = Vec::new();
    let mut section = None; // current section kind, or None outside them
    let mut origin_loc: Option<String> = None; // origin comment on the previous line
    let mut scope = ""; // pattern of the current pattern-specific block
    let mut target: Option<&str> = None; // target awaiting its recipe location
    let mut skip = false; // current entry is "# Not a target:"
    let mut phony = false; // current entry is a phony target
    let mut curdir = String::new(); // CURDIR with a trailing slash, once seen
    let mut cont = false; // previous line is a recipe line ending in a backslash

    for line in input.lines() {
        if let Some(kind) = section_kind(line) {
            section = kind;
            continue;
        }
        if line.starts_with('\t') || cont {
            cont = line.ends_with('\\');
            continue;
        }
        let Some(sec) = section else { continue };
        let prev = origin_loc.take();
        let kind = if phony { "phony" } else { sec };
        if let Some(loc) = origin(line) {
            origin_loc = Some(loc);
        } else if let Some(loc) = recipe(line) {
            if let Some(t) = target.take().filter(|_| !skip) {
                rows.push((kind, t.to_string(), loc));
            }
        } else if line == "# Not a target:" {
            skip = true;
        } else if line == "#  Phony target (prerequisite of .PHONY)." {
            phony = true;
        } else if line.is_empty() {
            if let Some(t) = target.take().filter(|_| !skip) {
                rows.push((kind, t.to_string(), "-".to_string()));
            }
            (skip, phony) = (false, false);
        } else if sec == "variable" {
            if let Some(loc) = prev {
                rows.push(("variable", varname(line).to_string(), loc));
                if let Some(dir) = line.strip_prefix("CURDIR := ") {
                    curdir = format!("{dir}/");
                }
            }
        } else if sec == "pattern-var" {
            match (prev, line.strip_prefix("# ")) {
                (Some(loc), Some(def)) => {
                    rows.push(("variable", format!("{scope}:{}", varname(def)), loc))
                }
                _ => {
                    if let Some(s) = line.strip_suffix(" :") {
                        scope = s;
                    }
                }
            }
        } else if !line.starts_with('#') {
            let (name, rest) = line.split_once(':').unwrap_or((line, ""));
            match prev {
                Some(loc) => rows.push(("variable", format!("{name}:{}", varname(rest)), loc)),
                None => target = Some(name),
            }
        }
    }

    let strip = |s: &str| s.strip_prefix(curdir.as_str()).unwrap_or(s).to_string();
    let mut rows: Vec<_> = rows
        .into_iter()
        .filter(|(k, _, _)| args.all || !matches!(*k, "target" | "pattern"))
        .map(|(k, n, loc)| (k, strip(&n), strip(&loc)))
        .collect();
    rows.sort();
    let w0 = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0);
    let w1 = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
    let mut out = BufWriter::new(io::stdout().lock());
    for (kind, name, loc) in rows {
        // A closed pipe (e.g. `| head`) is a normal way to stop.
        if writeln!(out, "{kind:<w0$}  {name:<w1$}  {loc}").is_err() {
            return Ok(());
        }
    }
    out.flush().or(Ok(()))
}
