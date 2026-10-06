use crate::units::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceRegion {
  pub span: ByteSpan,
  pub name: Vec<u8>,
  pub is_braced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
  Class,
  TypeAlias,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
  pub local: Vec<u8>,
  pub qualified: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseStatement {
  pub span: ByteSpan,
  pub kind: ImportKind,
  pub imports: Vec<Import>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
  pub namespaces: Vec<NamespaceRegion>,
  pub statements: Vec<UseStatement>,
}
