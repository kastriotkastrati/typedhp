use crate::units::ByteSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Function,
  Fn,
  Class,
  Interface,
  Trait,
  Enum,
  New,
  Namespace,
  Extends,
  Implements,
  Use,
  Const,
  Modifier,
  Name,
  Variable,
  Colon,
  DoubleColon,
  ObjectOperator,
  LessThan,
  Comma,
  Equal,
  As,
  NamespaceSeparator,
  OpenTag,
  Semicolon,
  Arrow,
  Ampersand,
  Ellipsis,
  CloseTag,
  OpenParen,
  CloseParen,
  OpenBracket,
  OpenAttribute,
  CloseBracket,
  OpenBrace,
  CloseBrace,
  Other,
}

impl Kind {
  pub fn is_opener(&self) -> bool {
    return matches!(
      self,
      Kind::OpenParen | Kind::OpenBracket | Kind::OpenAttribute | Kind::OpenBrace
    );
  }

  pub fn is_closer(&self) -> bool {
    return matches!(self, Kind::CloseParen | Kind::CloseBracket | Kind::CloseBrace);
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
  pub kind: Kind,
  pub span: ByteSpan,
  pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed {
  pub tokens: Vec<Token>,
  pub doc_comments: Vec<ByteSpan>,
}
