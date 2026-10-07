use crate::check::CheckKind;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
  pub globals: Vec<OsString>,
  pub workspace: Option<PathBuf>,
  pub command: Option<String>,
  pub arguments: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedCommand {
  Format,
  Check(CheckKind),
}
