//! The Grep tool: the lines of the files under a path that match a regular
//! expression, with optional context lines.
//!
//! The walk skips hidden entries and does not follow symlinks.

use std::ops::Range;
use std::path::{Path, PathBuf};

use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use walkdir::{DirEntry, WalkDir};

use super::{Sandbox, ToolError};

/// The most context lines a search shows around a match.
const MAX_CONTEXT: usize = 10_000;

/// The input of the Grep tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GrepInput {
    /// Regex pattern to search for.
    pattern: String,
    /// File or directory to search; the sandbox root when absent.
    path: Option<String>,
    /// Number of context lines around matches.
    context: Option<usize>,
}

/// Runs the Grep tool on `input`, inside `sandbox`.
pub(super) fn run(sandbox: &Sandbox, input: &GrepInput) -> Result<String, ToolError> {
    let regex = Regex::new(&input.pattern)?;
    let root = sandbox.resolve(input.path.as_deref().unwrap_or("."))?;
    let meta = std::fs::metadata(&root).map_err(ToolError::io(&root))?;
    let around = input.context.unwrap_or(0).min(MAX_CONTEXT);

    // Paths print relative to the searched directory, or a file's parent.
    let base = if meta.is_dir() {
        root.clone()
    } else {
        root.parent().unwrap_or(&root).to_path_buf()
    };

    let mut hits = Vec::new();
    for file in files_under(&root) {
        // A file that is not UTF-8 text, or cannot be read, has no hits.
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        let name = file
            .strip_prefix(&base)
            .unwrap_or(&file)
            .display()
            .to_string();
        hits.extend(file_hits(&name, &content, &regex, around));
    }

    if hits.is_empty() {
        return Ok("No matches found".into());
    }
    Ok(hits.join("\n"))
}

/// The regular files under `root`, sorted, skipping hidden entries and
/// symlinks; `root` itself when it is a file.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let visible = |entry: &DirEntry| entry.depth() == 0 || !is_hidden(entry);
    let mut files: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_entry(visible)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(DirEntry::into_path)
        .collect();
    files.sort();
    files
}

/// Whether the entry's name starts with a dot.
fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}

/// The output lines for one file: `name:N:line` for a match, `name-N-line`
/// for a context line, `--` between windows that do not touch.
fn file_hits(name: &str, content: &str, regex: &Regex, around: usize) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let matches: Vec<usize> = (0..lines.len())
        .filter(|&i| regex.is_match(lines[i]))
        .collect();

    let mut hits = Vec::new();
    for window in windows(&matches, around, lines.len()) {
        if !hits.is_empty() {
            hits.push("--".to_string());
        }
        for index in window {
            let marker = if matches.binary_search(&index).is_ok() {
                ':'
            } else {
                '-'
            };
            hits.push(format!(
                "{name}{marker}{}{marker}{}",
                index + 1,
                lines[index]
            ));
        }
    }
    hits
}

/// The line ranges to show: each match widened by `context` lines on both
/// sides, clamped to `len`, with touching or overlapping ranges merged.
fn windows(matches: &[usize], around: usize, len: usize) -> Vec<Range<usize>> {
    let mut windows: Vec<Range<usize>> = Vec::new();
    for &index in matches {
        let window = index.saturating_sub(around)..(index + around + 1).min(len);
        match windows.last_mut() {
            Some(last) if window.start <= last.end => last.end = window.end,
            _ => windows.push(window),
        }
    }
    windows
}

#[cfg(test)]
mod tests;
