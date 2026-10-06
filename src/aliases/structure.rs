use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
  Text(Vec<u8>),
  Param(usize),
  Alias { name: Vec<u8>, arguments: Vec<Vec<Segment>> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasDefinition {
  pub(crate) defaults: Vec<Option<Vec<Segment>>>,
  pub(crate) body: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAlias {
  pub name: Vec<u8>,
  pub line: usize,
  pub(crate) definition: AliasDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasTable {
  pub(crate) definitions: HashMap<Vec<u8>, AliasDefinition>,
}
