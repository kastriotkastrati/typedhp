use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
  Text(Vec<u8>),
  Alias(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAlias {
  pub name: Vec<u8>,
  pub line: usize,
  pub(crate) segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasTable {
  pub(crate) definitions: HashMap<Vec<u8>, Vec<Segment>>,
}
