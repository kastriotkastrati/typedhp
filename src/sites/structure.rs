use crate::lex::Token;
use crate::names::Names;
use crate::types::Declarations;
use crate::types::Scope;
use crate::types::TypeParam;
use crate::types::TypeParamList;
use crate::units::ByteSpan;

pub struct Code<'a> {
  pub source: &'a [u8],
  pub tokens: &'a [Token],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clause {
  Extends,
  Implements,
  TraitUse,
  Turbofish,
}

pub enum AngleGroup {
  Declaration {
    owner: usize,
    list: TypeParamList,
  },
  Arguments {
    clause: Clause,
    owner: usize,
    named: Option<ByteSpan>,
    arguments: ByteSpan,
    removal: ByteSpan,
  },
}

impl AngleGroup {
  pub fn removal(&self) -> ByteSpan {
    return match self {
      AngleGroup::Declaration { list, .. } => list.span,
      AngleGroup::Arguments { removal, .. } => *removal,
    };
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
  pub type_span: ByteSpan,
  pub variable: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionHeader {
  pub anchor: usize,
  pub type_params: Vec<TypeParam>,
  pub parameters: Vec<Parameter>,
  pub return_type: Option<ByteSpan>,
  pub scope: Option<Scope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassHeader {
  pub anchor: usize,
  pub type_params: Vec<TypeParam>,
  pub extends: Vec<ByteSpan>,
  pub implements: Vec<ByteSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitUse {
  pub anchor: usize,
  pub traits: Vec<ByteSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
  pub anchor: usize,
  pub type_span: ByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Construction {
  pub anchor: usize,
  pub variable: ByteSpan,
  pub class: ByteSpan,
  pub arguments: ByteSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasDeclaration {
  pub removal: ByteSpan,
  pub name: Vec<u8>,
  pub params: Vec<TypeParam>,
  pub body: ByteSpan,
}

pub struct Sites {
  pub removals: Vec<ByteSpan>,
  pub declarations: Declarations,
  pub functions: Vec<FunctionHeader>,
  pub classes: Vec<ClassHeader>,
  pub trait_uses: Vec<TraitUse>,
  pub members: Vec<Member>,
  pub constructions: Vec<Construction>,
  pub alias_declarations: Vec<AliasDeclaration>,
  pub names: Names,
}
