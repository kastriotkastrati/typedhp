mod structure;

pub use structure::Kind;
pub use structure::Lexed;
pub use structure::Token;

use crate::strip::StripError;
use crate::units::ByteSpan;
use crate::units::to_offset;
use itertools::Itertools;
use mago_database::file::FileId;
use mago_syntax::error::SyntaxError;
use mago_syntax::lexer::Lexer;
use mago_syntax::settings::LexerSettings;
use mago_syntax::token::TokenKind;
use mago_syntax_core::input::Input;

fn kind_of(token_kind: TokenKind) -> Option<Kind> {
  return match token_kind {
    TokenKind::Whitespace
    | TokenKind::SingleLineComment
    | TokenKind::HashComment
    | TokenKind::MultiLineComment
    | TokenKind::DocBlockComment => None,
    TokenKind::Function => Some(Kind::Function),
    TokenKind::Fn => Some(Kind::Fn),
    TokenKind::Class => Some(Kind::Class),
    TokenKind::Interface => Some(Kind::Interface),
    TokenKind::Trait => Some(Kind::Trait),
    TokenKind::Enum => Some(Kind::Enum),
    TokenKind::New => Some(Kind::New),
    TokenKind::Namespace => Some(Kind::Namespace),
    TokenKind::Extends => Some(Kind::Extends),
    TokenKind::Implements => Some(Kind::Implements),
    TokenKind::Use => Some(Kind::Use),
    TokenKind::Const => Some(Kind::Const),
    TokenKind::Public
    | TokenKind::Protected
    | TokenKind::Private
    | TokenKind::PublicSet
    | TokenKind::ProtectedSet
    | TokenKind::PrivateSet
    | TokenKind::Static
    | TokenKind::Readonly
    | TokenKind::Var
    | TokenKind::Abstract
    | TokenKind::Final => Some(Kind::Modifier),
    TokenKind::Identifier
    | TokenKind::QualifiedIdentifier
    | TokenKind::FullyQualifiedIdentifier => Some(Kind::Name),
    TokenKind::Variable => Some(Kind::Variable),
    TokenKind::Colon => Some(Kind::Colon),
    TokenKind::ColonColon => Some(Kind::DoubleColon),
    TokenKind::MinusGreaterThan | TokenKind::QuestionMinusGreaterThan => Some(Kind::ObjectOperator),
    TokenKind::LessThan => Some(Kind::LessThan),
    TokenKind::Comma => Some(Kind::Comma),
    TokenKind::Equal => Some(Kind::Equal),
    TokenKind::As => Some(Kind::As),
    TokenKind::NamespaceSeparator => Some(Kind::NamespaceSeparator),
    TokenKind::OpenTag | TokenKind::ShortOpenTag => Some(Kind::OpenTag),
    TokenKind::Semicolon => Some(Kind::Semicolon),
    TokenKind::EqualGreaterThan => Some(Kind::Arrow),
    TokenKind::Ampersand => Some(Kind::Ampersand),
    TokenKind::DotDotDot => Some(Kind::Ellipsis),
    TokenKind::CloseTag => Some(Kind::CloseTag),
    TokenKind::LeftParenthesis => Some(Kind::OpenParen),
    TokenKind::RightParenthesis => Some(Kind::CloseParen),
    TokenKind::LeftBracket => Some(Kind::OpenBracket),
    TokenKind::HashLeftBracket => Some(Kind::OpenAttribute),
    TokenKind::RightBracket => Some(Kind::CloseBracket),
    TokenKind::LeftBrace | TokenKind::DollarLeftBrace => Some(Kind::OpenBrace),
    TokenKind::RightBrace => Some(Kind::CloseBrace),
    _ => Some(Kind::Other),
  };
}

pub fn lex(source: &[u8]) -> Result<Lexed, StripError> {
  let input = Input::new(FileId::zero(), source);
  let mut lexer = Lexer::new(input, LexerSettings::default());
  let lexed = std::iter::from_fn(|| lexer.advance()).collect::<Result<Vec<_>, SyntaxError>>();
  let raw_tokens = match lexed {
    Ok(tokens) => tokens,
    Err(error) => {
      let position = match error {
        SyntaxError::UnexpectedToken(_, _, position) => position,
        SyntaxError::UnrecognizedToken(_, _, position) => position,
        SyntaxError::UnexpectedEndOfFile(_, position) => position,
        SyntaxError::RecursionLimitExceeded(_, position) => position,
      };

      let offset =
        to_offset(position.offset).map_err(|reason| StripError::at(source, 0, reason))?;

      return Err(StripError::at(source, offset, "PHP syntax error"));
    }
  };

  let doc_comments = raw_tokens
    .iter()
    .filter(|token| token.kind == TokenKind::DocBlockComment)
    .map(|token| {
      let start = to_offset(token.start.offset)?;
      return Ok(ByteSpan { start, end: start + token.value.len() });
    })
    .collect::<Result<Vec<_>, &'static str>>()
    .map_err(|reason| StripError::at(source, 0, reason))?;

  let significant = raw_tokens.iter().filter_map(|token| {
    let kind = kind_of(token.kind)?;
    return Some((kind, token));
  });

  let spanned = significant
    .map(|(kind, token)| {
      let start = to_offset(token.start.offset)?;
      let end = start + token.value.len();
      return Ok((kind, ByteSpan { start, end }));
    })
    .collect::<Result<Vec<_>, &'static str>>()
    .map_err(|reason| StripError::at(source, 0, reason))?;

  let levels = std::iter::successors(Some((0usize, 0usize)), |(index, level)| {
    let (kind, _) = spanned.get(*index)?;
    let next_level = match (kind.is_opener(), kind.is_closer()) {
      (true, _) => Some(level + 1),
      (false, true) => level.checked_sub(1),
      (false, false) => Some(*level),
    };

    return next_level.map(|next_level| (index + 1, next_level));
  })
  .map(|(_, level)| level)
  .collect::<Vec<_>>();

  let unopened_closer = spanned.get(levels.len() - 1);
  if let Some((_, span)) = unopened_closer {
    return Err(StripError::at(source, span.start, "closing bracket without an opening bracket"));
  }

  let depths = levels.iter().tuple_windows().map(|(before, after)| *before.min(after));
  let tokens = spanned.iter().zip(depths).map(|((kind, span), depth)| Token {
    kind: *kind,
    span: *span,
    depth,
  });

  return Ok(Lexed { tokens: tokens.collect(), doc_comments });
}
