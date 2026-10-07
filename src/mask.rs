mod structure;

pub use structure::Masked;

use crate::lex::Kind;
use crate::lex::lex;
use crate::names::ImportKind;
use crate::sites::AngleGroup;
use crate::sites::Clause;
use crate::sites::find_sites;
use crate::strip::Edit;
use crate::strip::StripError;
use crate::strip::guard_size;
use crate::strip::splice;
use crate::units::ByteSpan;
use itertools::Itertools;
use structure::Cover;
use structure::Piece;
use structure::Placement;

fn is_name_byte(byte: &u8) -> bool {
  return byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'\\' || *byte >= 0x80;
}

fn is_plain_type(text: &[u8]) -> bool {
  let parts = text.split(|byte| byte.is_ascii_whitespace() || b"|&()?".contains(byte));
  let mut words = parts.filter(|part| !part.is_empty());
  return words.all(|word| {
    let starts_like_name = word.first().is_some_and(|byte| !byte.is_ascii_digit());
    let is_reserved = word.eq_ignore_ascii_case(b"list");
    return starts_like_name && !is_reserved && word.iter().all(is_name_byte);
  });
}

fn indent_span(text: &[u8], offset: usize) -> ByteSpan {
  let line_start =
    text[..offset].iter().rposition(|byte| *byte == b'\n').map_or(0, |newline| newline + 1);

  let width = text[line_start..].iter().take_while(|byte| matches!(byte, b' ' | b'\t')).count();
  return ByteSpan { start: line_start, end: line_start + width };
}

fn reindent(text: &[u8], old_indent: &[u8], new_indent: &[u8]) -> Vec<u8> {
  let lines = text.split(|byte| *byte == b'\n').enumerate().map(|(number, line)| {
    let is_continuation = number > 0 && !line.is_empty();
    let rest = line.strip_prefix(old_indent).filter(|_| is_continuation);
    return match rest {
      Some(rest) => [new_indent, rest].concat(),
      None => line.to_vec(),
    };
  });

  return lines.collect::<Vec<_>>().join(b"\n".as_slice());
}

fn single_spaced(text: &[u8]) -> Vec<u8> {
  let words = text.split(u8::is_ascii_whitespace).filter(|word| !word.is_empty());
  return words.collect_vec().join(b" ".as_slice());
}

fn tidy_spacing(text: &[u8]) -> Vec<u8> {
  let lines = text.split(|byte| *byte == b'\n').map(|line| {
    let indent_width = line.iter().take_while(|byte| matches!(byte, b' ' | b'\t')).count();
    let (indent, rest) = line.split_at(indent_width);
    return [indent, &single_spaced(rest)].concat();
  });

  return lines.collect_vec().join(b"\n".as_slice());
}

pub fn mask(source: &[u8]) -> Result<Masked, StripError> {
  guard_size(source)?;
  let lexed = lex(source)?;
  let tokens = &lexed.tokens;
  let sites = find_sites(source, tokens)?;
  let callables =
    tokens.iter().positions(|token| matches!(token.kind, Kind::Function | Kind::Fn)).collect_vec();

  let group_covers = sites.groups.iter().map(|group| {
    let removal = group.removal();
    let fail = |reason| StripError::at(source, removal.start, reason);
    match group {
      AngleGroup::Declaration { owner, list } => {
        let after_keyword = owner + 1;
        let returns_reference =
          tokens.get(after_keyword).is_some_and(|token| token.kind == Kind::Ampersand);

        let name_index = if returns_reference { after_keyword + 1 } else { after_keyword };
        let name = tokens.get(name_index).ok_or_else(|| fail("expected a name"))?;
        let is_anonymous = name.span.start == list.span.start;
        if is_anonymous {
          let nth = callables.partition_point(|index| index < owner);
          return Ok(Cover { span: list.span, placement: Placement::BeforeParameters { nth } });
        }

        let span = ByteSpan { start: name.span.start, end: list.span.end };
        return Ok(Cover { span, placement: Placement::Inline { sigil: b"" } });
      }
      AngleGroup::Arguments { clause: Clause::Turbofish, owner, removal, .. } => {
        let previous = owner.checked_sub(1).and_then(|index| tokens.get(index));
        let previous = previous.ok_or_else(|| fail("expected a name before type arguments"))?;
        let previous_text = &source[previous.span.start..previous.span.end];
        let is_variable = previous.kind == Kind::Variable;
        let is_name = previous_text.iter().all(is_name_byte);
        let is_callable = is_variable || is_name;
        if !is_callable {
          return Err(fail("typedhp cannot format type arguments after this expression"));
        }

        let sigil: &'static [u8] = if is_variable { b"$" } else { b"" };
        let span = ByteSpan { start: previous.span.start, end: removal.end };
        return Ok(Cover { span, placement: Placement::Inline { sigil } });
      }
      AngleGroup::Arguments { named, removal, .. } => {
        let named = named.ok_or_else(|| fail("expected a name before type arguments"))?;
        let span = ByteSpan { start: named.start, end: removal.end };
        return Ok(Cover { span, placement: Placement::Inline { sigil: b"" } });
      }
    }
  });

  let type_imports =
    sites.names.statements.iter().filter(|statement| statement.kind == ImportKind::TypeAlias);

  let import_covers = type_imports.map(|statement| {
    let first = statement.imports.first();
    let fail = || StripError::at(source, statement.span.start, "expected a name after `use type`");
    let sort_key = first.ok_or_else(fail)?.qualified.clone();
    return Ok(Cover { span: statement.span, placement: Placement::TypeImport { sort_key } });
  });

  let typed_spans = sites.type_spans().into_iter().filter(|span| {
    return !is_plain_type(&source[span.start..span.end]);
  });

  let type_covers =
    typed_spans.map(|span| Ok(Cover { span, placement: Placement::Inline { sigil: b"" } }));

  let alias_covers = sites.alias_declarations.iter().map(|declaration| {
    let placement = Placement::AliasDeclaration { body: declaration.body };
    return Ok(Cover { span: declaration.removal, placement });
  });

  let covers = group_covers
    .chain(import_covers)
    .chain(type_covers)
    .chain(alias_covers)
    .collect::<Result<Vec<_>, StripError>>()?;

  let longest_marker_run = source.iter().positions(|byte| *byte == b'_').map(|underscore| {
    let letters = source[underscore + 1..].iter().take_while(|byte| **byte == b'q').count();
    let follower = source.get(underscore + 1 + letters);
    let is_followed_by_digit = follower.is_some_and(u8::is_ascii_digit);
    return if is_followed_by_digit { letters } else { 0 };
  });

  let marker_letters = longest_marker_run.fold(0, usize::max) + 1;
  let marker = [b"_".as_slice(), &vec![b'q'; marker_letters]].concat();
  let edits = covers.iter().enumerate().map(|(index, cover)| {
    let original = &source[cover.span.start..cover.span.end];
    let name = [marker.as_slice(), index.to_string().as_bytes()].concat();
    let replacement = match &cover.placement {
      Placement::Inline { sigil } => {
        let words = original.split(u8::is_ascii_whitespace).filter(|word| !word.is_empty());
        let word_lengths = words.map(<[u8]>::len).collect_vec();
        let one_line_width =
          word_lengths.iter().sum::<usize>() + word_lengths.len().saturating_sub(1);

        let padding = one_line_width.saturating_sub(sigil.len() + name.len());
        [*sigil, name.as_slice(), &vec![b'_'; padding]].concat()
      }
      Placement::AliasDeclaration { .. } => [name.as_slice(), b";"].concat(),
      Placement::TypeImport { sort_key } => {
        [b"use ", sort_key.as_slice(), b"\\", name.as_slice(), b";"].concat()
      }
      Placement::BeforeParameters { .. } => Vec::new(),
    };

    return Edit { span: cover.span, replacement };
  });

  let code = splice(source, &edits.collect_vec())?;
  let pieces = covers.into_iter().map(|cover| {
    let written = &source[cover.span.start..cover.span.end];
    let original = match &cover.placement {
      Placement::AliasDeclaration { body } => {
        let head = single_spaced(&source[cover.span.start..body.start]);
        let type_text = &source[body.start..body.end];
        let ending = source[body.end..cover.span.end].trim_ascii();
        [head.as_slice(), b" ", type_text, ending].concat()
      }
      Placement::TypeImport { .. } => tidy_spacing(written),
      Placement::Inline { .. } | Placement::BeforeParameters { .. } => written.to_vec(),
    };

    let indent = indent_span(source, cover.span.start);
    let indent = source[indent.start..indent.end].to_vec();
    return Piece { original, indent, placement: cover.placement };
  });

  return Ok(Masked { code, marker, pieces: pieces.collect() });
}

pub fn unmask(formatted: &[u8], masked: &Masked) -> Result<Vec<u8>, StripError> {
  let marker = masked.marker.as_slice();
  let found = formatted.windows(marker.len()).positions(|window| window == marker);
  let placeholders = found
    .filter_map(|start| {
      let digits_start = start + marker.len();
      let digit_count =
        formatted[digits_start..].iter().take_while(|byte| byte.is_ascii_digit()).count();

      let digits = &formatted[digits_start..digits_start + digit_count];
      let index = std::str::from_utf8(digits).ok()?.parse::<usize>().ok()?;
      let padding_start = digits_start + digit_count;
      let padding = formatted[padding_start..].iter().take_while(|byte| **byte == b'_').count();
      return Some((index, ByteSpan { start, end: padding_start + padding }));
    })
    .into_group_map();

  let lost = |offset: usize| {
    return StripError::at(
      formatted,
      offset,
      "the formatter changed code that typedhp hid from it",
    );
  };

  let hidden_count = masked
    .pieces
    .iter()
    .filter(|piece| !matches!(piece.placement, Placement::BeforeParameters { .. }))
    .count();

  let found_count = placeholders.values().map(Vec::len).sum::<usize>();
  let is_complete = found_count == hidden_count;
  if !is_complete {
    return Err(lost(0));
  }

  let lexed = lex(formatted)?;
  let callables = lexed
    .tokens
    .iter()
    .positions(|token| matches!(token.kind, Kind::Function | Kind::Fn))
    .collect_vec();

  let edits = masked.pieces.iter().enumerate().map(|(index, piece)| {
    if let Placement::BeforeParameters { nth } = piece.placement {
      let keyword_index = *callables.get(nth).ok_or_else(|| lost(0))?;
      let next_index = keyword_index + 1;
      let returns_reference =
        lexed.tokens.get(next_index).is_some_and(|token| token.kind == Kind::Ampersand);

      let parameters_index = if returns_reference { next_index + 1 } else { next_index };
      let parameters = lexed.tokens.get(parameters_index).ok_or_else(|| lost(0))?;
      let at = parameters.span.start;

      return Ok(Edit {
        span: ByteSpan { start: at, end: at },
        replacement: piece.original.clone(),
      });
    }

    let spans = placeholders.get(&index).map(Vec::as_slice);
    let Some([span]) = spans else {
      return Err(lost(0));
    };

    let indent = indent_span(formatted, span.start);
    let replacement =
      reindent(&piece.original, &piece.indent, &formatted[indent.start..indent.end]);

    if let Placement::Inline { sigil } = piece.placement {
      let start = span.start.checked_sub(sigil.len()).ok_or_else(|| lost(span.start))?;
      let has_sigil = &formatted[start..span.start] == sigil;
      if !has_sigil {
        return Err(lost(span.start));
      }

      return Ok(Edit { span: ByteSpan { start, end: span.end }, replacement });
    }

    if let Placement::AliasDeclaration { .. } = piece.placement {
      let ends_statement = formatted[span.end..].starts_with(b";");
      if !ends_statement {
        return Err(lost(span.start));
      }

      return Ok(Edit { span: ByteSpan { start: span.start, end: span.end + 1 }, replacement });
    }

    let starts_statement = formatted[indent.end..span.start].starts_with(b"use");
    let semicolon = formatted[span.end..].iter().position(|byte| *byte == b';');
    let Some(semicolon) = semicolon.filter(|_| starts_statement) else {
      return Err(lost(span.start));
    };

    let statement = ByteSpan { start: indent.end, end: span.end + semicolon + 1 };
    return Ok(Edit { span: statement, replacement });
  });

  return splice(formatted, &edits.collect::<Result<Vec<_>, StripError>>()?);
}
