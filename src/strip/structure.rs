use crate::units::ByteSpan;
use crate::units::line_at;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripError {
  pub line: usize,
  pub reason: &'static str,
}

impl StripError {
  pub fn at(source: &[u8], offset: usize, reason: &'static str) -> StripError {
    return StripError { line: line_at(source, offset), reason };
  }
}

impl std::fmt::Display for StripError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(formatter, "line {}: {}", self.line, self.reason);
  }
}

impl std::error::Error for StripError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
  pub span: ByteSpan,
  pub replacement: Vec<u8>,
}
