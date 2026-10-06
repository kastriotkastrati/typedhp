mod structure;

pub use structure::Import;
pub use structure::ImportKind;
pub use structure::Names;
pub use structure::NamespaceRegion;
pub use structure::UseStatement;

use crate::lex::Kind;
use crate::lex::Token;
use crate::sites::Code;
use crate::strip::StripError;
use crate::units::ByteSpan;

fn starts_statement(previous: Option<Kind>) -> bool {
  return matches!(
    previous,
    None | Some(Kind::Semicolon | Kind::OpenBrace | Kind::CloseBrace | Kind::OpenTag)
  );
}

fn token_text<'a>(code: &Code<'a>, token: &Token) -> &'a [u8] {
  return &code.source[token.span.start..token.span.end];
}

fn text<'a>(code: &Code<'a>, index: usize) -> &'a [u8] {
  return token_text(code, &code.tokens[index]);
}

fn kind_at(code: &Code<'_>, index: usize) -> Option<Kind> {
  return code.tokens.get(index).map(|token| token.kind);
}

fn last_segment(name: &[u8]) -> &[u8] {
  return name.rsplit(|byte| *byte == b'\\').next().unwrap_or(name);
}

fn trim_leading_separator(name: &[u8]) -> &[u8] {
  return name.strip_prefix(b"\\").unwrap_or(name);
}

fn namespace_regions(code: &Code<'_>) -> Result<Vec<NamespaceRegion>, StripError> {
  let declarations = (0..code.tokens.len())
    .filter(|index| {
      let is_keyword = code.tokens[*index].kind == Kind::Namespace;
      let previous = index.checked_sub(1).and_then(|previous| kind_at(code, previous));
      let opens = matches!(kind_at(code, index + 1), Some(Kind::Name | Kind::OpenBrace));
      return is_keyword && starts_statement(previous) && opens;
    })
    .collect::<Vec<_>>();

  let regions = declarations.iter().enumerate().map(|(position, index)| {
    let keyword = code.tokens[*index];
    let is_named = kind_at(code, index + 1) == Some(Kind::Name);
    let name =
      if is_named { trim_leading_separator(text(code, index + 1)).to_vec() } else { Vec::new() };

    let brace_index = if is_named { index + 2 } else { index + 1 };
    let is_braced = kind_at(code, brace_index) == Some(Kind::OpenBrace);
    if is_braced {
      let open = code.tokens[brace_index];
      let close = code.tokens[brace_index + 1..]
        .iter()
        .find(|token| token.kind == Kind::CloseBrace && token.depth == open.depth)
        .ok_or_else(|| StripError::at(code.source, keyword.span.start, "unclosed namespace"))?;

      return Ok(NamespaceRegion {
        span: ByteSpan { start: open.span.start, end: close.span.end },
        name,
        is_braced,
      });
    }

    let end = match declarations.get(position + 1) {
      Some(next) => code.tokens[*next].span.start,
      None => code.source.len(),
    };

    return Ok(NamespaceRegion {
      span: ByteSpan { start: keyword.span.start, end },
      name,
      is_braced,
    });
  });

  let regions = regions.collect::<Result<Vec<_>, StripError>>()?;
  let has_namespaces = !regions.is_empty();
  if has_namespaces {
    return Ok(regions);
  }

  let whole_file = ByteSpan { start: 0, end: code.source.len() };
  return Ok(vec![NamespaceRegion { span: whole_file, name: Vec::new(), is_braced: false }]);
}

fn region_at(regions: &[NamespaceRegion], offset: usize) -> Option<&NamespaceRegion> {
  return regions.iter().find(|region| region.span.contains(offset));
}

fn imports_between(code: &Code<'_>, start: usize, end: usize) -> Vec<Import> {
  let tokens = &code.tokens[start..end];
  let kinds = tokens.iter().map(|token| token.kind).collect::<Vec<_>>();
  let opens_group = kinds.starts_with(&[Kind::Name, Kind::NamespaceSeparator, Kind::OpenBrace]);
  let group_close = kinds.iter().rposition(|kind| *kind == Kind::CloseBrace);
  let grouped_items = group_close.and_then(|close| tokens.get(3..close)).filter(|_| opens_group);
  let (prefix, items) = match grouped_items {
    Some(inside) => (Some(trim_leading_separator(token_text(code, &tokens[0]))), inside),
    None => (None, tokens),
  };

  let imports = items.split(|token| token.kind == Kind::Comma).filter_map(|item| {
    let item_kinds = item.iter().map(|token| token.kind).collect::<Vec<_>>();
    let written = item.first().map(|token| trim_leading_separator(token_text(code, token)))?;
    let local = match item_kinds.as_slice() {
      [Kind::Name] => last_segment(written),
      [Kind::Name, Kind::As, Kind::Name] => token_text(code, &item[2]),
      _ => return None,
    };

    let qualified = match prefix {
      Some(group) => [group, b"\\", written].concat(),
      None => written.to_vec(),
    };

    return Some(Import { local: local.to_vec(), qualified });
  });

  return imports.collect();
}

fn use_statements(
  code: &Code<'_>,
  regions: &[NamespaceRegion],
) -> Result<Vec<UseStatement>, StripError> {
  let statements = (0..code.tokens.len()).filter_map(|index| {
    let keyword = code.tokens[index];
    let is_use = keyword.kind == Kind::Use;
    let region = region_at(regions, keyword.span.start);
    let import_depth = region.map(|region| usize::from(region.is_braced));
    let is_top_level = import_depth == Some(keyword.depth);
    let captures_variables = kind_at(code, index + 1) == Some(Kind::OpenParen);
    let is_import = is_use && is_top_level && !captures_variables;
    if !is_import {
      return None;
    }

    let semicolon = code.tokens[index..]
      .iter()
      .position(|token| token.kind == Kind::Semicolon && token.depth == keyword.depth)
      .map(|offset| index + offset);

    let Some(semicolon) = semicolon else {
      return Some(Err(StripError::at(
        code.source,
        keyword.span.start,
        "unterminated `use` statement",
      )));
    };

    let marker = kind_at(code, index + 1);
    let imports_functions = matches!(marker, Some(Kind::Function | Kind::Const));
    if imports_functions {
      return None;
    }

    let imports_types = marker == Some(Kind::Name)
      && text(code, index + 1) == b"type"
      && kind_at(code, index + 2) == Some(Kind::Name);

    let (kind, first_item) = if imports_types {
      (ImportKind::TypeAlias, index + 2)
    } else {
      (ImportKind::Class, index + 1)
    };

    let span = ByteSpan { start: keyword.span.start, end: code.tokens[semicolon].span.end };
    let imports = imports_between(code, first_item, semicolon);
    return Some(Ok(UseStatement { span, kind, imports }));
  });

  return statements.collect();
}

pub fn find_names(code: &Code<'_>) -> Result<Names, StripError> {
  let namespaces = namespace_regions(code)?;
  let statements = use_statements(code, &namespaces)?;
  return Ok(Names { namespaces, statements });
}

pub fn namespace_at(names: &Names, offset: usize) -> &[u8] {
  return match region_at(&names.namespaces, offset) {
    Some(region) => &region.name,
    None => b"",
  };
}

fn imports_at(names: &Names, offset: usize, kind: ImportKind) -> impl Iterator<Item = &Import> {
  let region = region_at(&names.namespaces, offset).map(|region| region.span);
  return names
    .statements
    .iter()
    .filter(move |statement| {
      let is_kind = statement.kind == kind;
      let shares_region = region.is_some_and(|span| span.contains(statement.span.start));
      return is_kind && shares_region;
    })
    .flat_map(|statement| statement.imports.iter());
}

pub fn qualify(namespace: &[u8], name: &[u8]) -> Vec<u8> {
  let is_global = namespace.is_empty();
  if is_global {
    return [b"\\", name].concat();
  }

  return [b"\\", namespace, b"\\", name].concat();
}

pub fn resolve_class(names: &Names, offset: usize, written: &[u8]) -> Vec<u8> {
  let is_fully_qualified = written.starts_with(b"\\");
  if is_fully_qualified {
    return written.to_vec();
  }

  let separator = written.iter().position(|byte| *byte == b'\\');
  let (first, rest) = match separator {
    Some(position) => written.split_at(position),
    None => (written, b"".as_slice()),
  };

  let import = imports_at(names, offset, ImportKind::Class)
    .find(|import| import.local.eq_ignore_ascii_case(first));

  return match import {
    Some(import) => [b"\\", import.qualified.as_slice(), rest].concat(),
    None => qualify(namespace_at(names, offset), written),
  };
}

pub fn imported_alias<'a>(names: &'a Names, offset: usize, written: &[u8]) -> Option<&'a [u8]> {
  let import =
    imports_at(names, offset, ImportKind::TypeAlias).find(|import| import.local == written);

  return import.map(|import| import.qualified.as_slice());
}
