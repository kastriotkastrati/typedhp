use crate::units::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
  Inline { sigil: &'static [u8] },
  AliasDeclaration { body: ByteSpan },
  TypeImport { sort_key: Vec<u8> },
  BeforeParameters { nth: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
  pub span: ByteSpan,
  pub placement: Placement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
  pub original: Vec<u8>,
  pub indent: Vec<u8>,
  pub placement: Placement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
  pub code: Vec<u8>,
  pub(crate) marker: Vec<u8>,
  pub(crate) pieces: Vec<Piece>,
}
