use crate::units::ByteSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Member {
  Named(Vec<u8>),
  Intersection(Vec<Vec<u8>>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Erased {
  Mixed,
  Members(Vec<Member>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erasure {
  pub erased: Erased,
  pub extended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variance {
  Invariant,
  Covariant,
  Contravariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParam {
  pub name: Vec<u8>,
  pub bound: Option<ByteSpan>,
  pub default: Option<ByteSpan>,
  pub variance: Variance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
  pub name: ByteSpan,
  pub span: ByteSpan,
  pub arguments: Vec<ByteSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParamList {
  pub span: ByteSpan,
  pub params: Vec<TypeParam>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
  pub span: ByteSpan,
  pub params: Vec<TypeParam>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declarations {
  pub scopes: Vec<Scope>,
  pub aliases: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeParamUse {
  Resolve,
  Mixed,
}

pub struct EraseContext<'a> {
  pub source: &'a [u8],
  pub declarations: &'a Declarations,
  pub position: usize,
  pub type_param_use: TypeParamUse,
}
