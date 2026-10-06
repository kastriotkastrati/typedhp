use std::ffi::OsString;
use std::path::PathBuf;
use typedhp::StripError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripInput {
  File,
  Stdin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
  Strip { path: PathBuf, input: StripInput },
  Install { folder: PathBuf },
  Check { mago_arguments: Vec<OsString> },
}

#[derive(Debug)]
pub enum Failure {
  Usage,
  Read { path: PathBuf, error: std::io::Error },
  Strip { path: PathBuf, error: StripError },
  WriteOutput { error: std::io::Error },
  InstallFolder { folder: PathBuf, reason: &'static str },
  Install { path: PathBuf, error: std::io::Error },
  Check { path: PathBuf, error: std::io::Error },
  Walk { error: ignore::Error },
  StartMago { program: PathBuf, error: std::io::Error },
  MagoStopped { program: PathBuf },
  MagoReport { error: serde_json::Error },
}
