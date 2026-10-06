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
