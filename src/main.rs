//! List targets, variables, and where they are defined, from a GNU make database.
//! See README.md for usage and output format.

use clap::Parser;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::process::{Command, Stdio, exit};

/// List targets, variables, and where they are defined, from a GNU make database.
///
/// Reads the output of `make -pRrq` from stdin. If stdin is a terminal, runs
/// `${MAKE:-make} -pRrq` in the current directory instead. With none of -v, -s,
/// -p, -t, or -f, shows global variables and phony targets.
#[derive(Parser)]
struct Args {
    /// Show global variables
    #[arg(short = 'v')]
    variables: bool,
    /// Show target- and pattern-specific variables
    #[arg(short = 's')]
    scoped: bool,
    /// Show phony targets
    #[arg(short = 'p')]
    phony: bool,
    /// Show pattern rules
    #[arg(short = 't')]
    patterns: bool,
    /// Show file targets
    #[arg(short = 'f')]
    files: bool,
    /// Show everything; same as -vsptf
    #[arg(short = 'a')]
    all: bool,
    /// Print TARGET's prerequisites as a tree instead of listing
    #[arg(long, value_name = "TARGET")]
    tree: Option<String>,
}

type Deps<'a> = HashMap<&'a str, Vec<&'a str>>;

/// Strips the CURDIR prefix so paths under it read as relative.
fn rel<'a>(s: &'a str, curdir: &str) -> &'a str {
    s.strip_prefix(curdir).unwrap_or(s)
}

/// Prints the prerequisites of `node`, one level deeper than `prefix`. A target
/// whose prerequisites were already printed is marked (*) and not expanded
/// again, which also stops cycles.
fn print_tree<'a>(
    out: &mut impl Write,
    deps: &Deps<'a>,
    node: &str,
    prefix: &str,
    seen: &mut HashSet<&'a str>,
    curdir: &str,
) -> io::Result<()> {
    let kids = deps.get(node).map_or(&[][..], Vec::as_slice);
    for (i, &kid) in kids.iter().enumerate() {
        let last = i + 1 == kids.len();
        let repeat = deps.get(kid).is_some_and(|v| !v.is_empty()) && !seen.insert(kid);
        let (branch, indent) = if last {
            ("└── ", "    ")
        } else {
            ("├── ", "│   ")
        };
        let mark = if repeat { " (*)" } else { "" };
        writeln!(out, "{prefix}{branch}{}{mark}", rel(kid, curdir))?;
        if !repeat {
            print_tree(out, deps, kid, &format!("{prefix}{indent}"), seen, curdir)?;
        }
    }
    Ok(())
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

/// Parses `# ORIGIN [private]`, optionally followed by `(from 'FILE', line N)`.
fn origin(line: &str) -> Option<String> {
    let rest = line.strip_prefix("# ")?;
    let (name, from) = match rest.split_once(" (from ") {
        Some((name, from)) => (name, Some(from)),
        None => (rest, None),
    };
    let name = name.strip_suffix(" private").unwrap_or(name);
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
    let mut args = Args::parse();
    if args.all {
        let a = &mut args;
        (a.variables, a.scoped, a.phony, a.patterns, a.files) = (true, true, true, true, true);
    }
    let default = !(args.variables || args.scoped || args.phony || args.patterns || args.files);
    let mut input = Vec::new();
    if io::stdin().is_terminal() {
        // Make's exit status says nothing about whether the database is usable,
        // so it is ignored; its stderr is passed through instead.
        let make = std::env::var_os("MAKE").unwrap_or("make".into());
        input = Command::new(&make)
            .arg("-pRrq")
            .stderr(Stdio::inherit())
            .output()
            .unwrap_or_else(|e| {
                eprintln!("mkdb: {}: {e}", make.display());
                exit(1)
            })
            .stdout;
    } else {
        io::stdin().read_to_end(&mut input)?;
    }
    let input = String::from_utf8_lossy(&input);

    let mut rows: Vec<(&str, String, String)> = Vec::new();
    let mut deps = Deps::new(); // prerequisites of each target, in order
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
                    rows.push(("scoped", format!("{scope}:{}", varname(def)), loc))
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
                Some(loc) => rows.push(("scoped", format!("{name}:{}", varname(rest)), loc)),
                None => {
                    target = Some(name);
                    // Double-colon rules add entries; implicit search can repeat names.
                    let v = deps.entry(name).or_default();
                    for p in rest.trim_start_matches(':').split_whitespace() {
                        if p != "|" && !v.contains(&p) {
                            v.push(p);
                        }
                    }
                }
            }
        }
    }

    let mut out = BufWriter::new(io::stdout().lock());
    if let Some(t) = &args.tree {
        let abs = format!("{curdir}{t}");
        let Some(root) = [t.as_str(), &abs]
            .into_iter()
            .find_map(|n| deps.get_key_value(n).map(|(k, _)| *k))
        else {
            eprintln!("mkdb: no such target: {t}");
            exit(1)
        };
        let mut seen = HashSet::from([root]);
        let _ = writeln!(out, "{}", rel(root, &curdir))
            .and_then(|_| print_tree(&mut out, &deps, root, "", &mut seen, &curdir))
            .and_then(|_| out.flush());
        return Ok(());
    }

    let strip = |s: &str| rel(s, &curdir).to_string();
    let mut rows: Vec<_> = rows
        .into_iter()
        .filter(|(k, _, _)| match *k {
            "variable" => args.variables || default,
            "scoped" => args.scoped,
            "phony" => args.phony || default,
            "pattern" => args.patterns,
            _ => args.files,
        })
        .map(|(k, n, loc)| (k, strip(&n), strip(&loc)))
        .collect();
    rows.sort();
    let w0 = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0);
    let w1 = rows.iter().map(|r| r.1.chars().count()).max().unwrap_or(0);
    for (kind, name, loc) in rows {
        // A closed pipe (e.g. `| head`) is a normal way to stop.
        if writeln!(out, "{kind:<w0$}  {name:<w1$}  {loc}").is_err() {
            return Ok(());
        }
    }
    out.flush().or(Ok(()))
}
