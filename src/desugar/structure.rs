use crate::aliases::AliasTable;
use crate::sites::Sites;
use crate::units::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desugared {
  pub code: Vec<u8>,
  pub lines: Vec<usize>,
  pub docblocks: Vec<ByteSpan>,
}

pub struct Annotation {
  pub anchor: usize,
  pub tags: Vec<Vec<u8>>,
}

pub struct TagContext<'a> {
  pub source: &'a [u8],
  pub sites: &'a Sites,
  pub table: &'a AliasTable,
}
