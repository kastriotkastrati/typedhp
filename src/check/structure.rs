use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Report {
  pub issues: Vec<Issue>,
}

#[derive(Debug, Deserialize)]
pub struct Issue {
  pub level: String,
  pub code: Option<String>,
  pub message: String,
  pub annotations: Vec<Annotation>,
  #[serde(default)]
  pub edits: Vec<(FileName, Vec<Change>)>,
}

#[derive(Debug, Deserialize)]
pub struct Change {
  pub range: ChangeRange,
  pub new_text: Vec<u8>,
  pub safety: Safety,
}

#[derive(Debug, Deserialize)]
pub struct ChangeRange {
  pub start: usize,
  pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
  Safe,
  PotentiallyUnsafe,
  Unsafe,
}

#[derive(Debug, Deserialize)]
pub struct Annotation {
  pub kind: String,
  pub span: Span,
}

#[derive(Debug, Deserialize)]
pub struct Span {
  pub file_id: FileName,
  pub start: Position,
}

#[derive(Debug, Deserialize)]
pub struct FileName {
  pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Position {
  pub offset: usize,
  pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
  Directory,
  Php,
  Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
  pub relative: PathBuf,
  pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Problem {
  pub path: PathBuf,
  pub line: usize,
  pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
  pub root: PathBuf,
  pub entries: Vec<Entry>,
  pub sources: Vec<(PathBuf, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
  Analyze,
  Lint,
  Guard,
}
