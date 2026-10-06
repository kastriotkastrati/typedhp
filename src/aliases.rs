mod structure;

pub use structure::AliasDefinition;
pub use structure::AliasTable;
pub use structure::Segment;
pub use structure::TypeAlias;

use crate::lex::lex;
use crate::names::imported_alias;
use crate::names::namespace_at;
use crate::names::qualify;
use crate::names::resolve_class;
use crate::sites::Sites;
use crate::sites::find_sites;
use crate::strip::StripError;
use crate::strip::guard_size;
use crate::types::Reference;
use crate::types::type_references;
use crate::units::ByteSpan;
use crate::units::line_at;
use itertools::Itertools;

pub fn alias_name(sites: &Sites, offset: usize, written: &[u8]) -> Option<Vec<u8>> {
  let declared = sites.alias_declarations.iter().find(|declaration| declaration.name == written);
  if let Some(declaration) = declared {
    let namespace = namespace_at(&sites.names, declaration.removal.start);
    return Some(qualify(namespace, written));
  }

  let imported = imported_alias(&sites.names, offset, written)?;
  return Some([b"\\", imported].concat());
}

fn covers(outer: ByteSpan, inner: ByteSpan) -> bool {
  return outer.start <= inner.start && inner.end <= outer.end;
}

pub fn segments(
  source: &[u8],
  span: ByteSpan,
  references: &[Reference],
  classify: &dyn Fn(ByteSpan, &[u8]) -> Segment,
) -> Vec<Segment> {
  let inside = references.iter().filter(|reference| covers(span, reference.name));
  let classified = inside
    .map(|reference| {
      let written = &source[reference.name.start..reference.name.end];
      let segment = classify(reference.name, written);
      return (reference, segment);
    })
    .collect::<Vec<_>>();

  let alias_spans = classified
    .iter()
    .filter(|(_, segment)| matches!(segment, Segment::Alias { .. }))
    .map(|(reference, _)| reference.span)
    .collect::<Vec<_>>();

  let outermost = classified.into_iter().filter(|(reference, _)| {
    let is_inside_alias = alias_spans.iter().any(|alias_span| {
      let is_other_alias = *alias_span != reference.span;
      return is_other_alias && covers(*alias_span, reference.name);
    });

    return !is_inside_alias;
  });

  let placed = outermost
    .map(|(reference, segment)| {
      let Segment::Alias { name, .. } = segment else {
        return (reference.name, segment);
      };

      let arguments = reference
        .arguments
        .iter()
        .map(|argument| segments(source, *argument, references, classify))
        .collect();

      return (reference.span, Segment::Alias { name, arguments });
    })
    .collect::<Vec<_>>();

  let covered_ends = placed.iter().map(|(covered, _)| covered.end);
  let covered_starts = placed.iter().map(|(covered, _)| covered.start);
  let text_starts = std::iter::once(span.start).chain(covered_ends);
  let text_ends = covered_starts.chain(std::iter::once(span.end));
  let texts = text_starts
    .zip(text_ends)
    .map(|(start, end)| Segment::Text(source[start..end].to_vec()))
    .collect::<Vec<_>>();

  let placed_segments = placed.into_iter().map(|(_, segment)| segment);
  return texts.into_iter().interleave(placed_segments).collect();
}

fn is_bare_name(text: &[u8]) -> bool {
  let is_name_byte = |byte: &u8| {
    let is_identifier_byte = byte.is_ascii_alphanumeric() || *byte == b'_' || *byte >= 0x80;
    return is_identifier_byte || matches!(byte, b'\\' | b'-');
  };

  return !text.is_empty() && text.iter().all(is_name_byte);
}

fn is_one_group(text: &[u8]) -> bool {
  let steps = text.iter().map(|byte| match byte {
    b'(' => 1_i64,
    b')' => -1,
    _ => 0,
  });

  let depths = steps.scan(0_i64, |depth, step| {
    *depth += step;
    return Some(*depth);
  });

  let first_close = depths.into_iter().position(|depth| depth == 0);
  let opens_first = text.first() == Some(&b'(');
  let closes_last = first_close == Some(text.len() - 1);
  return opens_first && closes_last;
}

fn grouped(text: Vec<u8>) -> Vec<u8> {
  let is_grouped = is_bare_name(&text) || is_one_group(&text);
  if is_grouped {
    return text;
  }

  return [b"(", text.as_slice(), b")"].concat();
}

pub fn expand(
  table: &AliasTable,
  pieces: &[Segment],
  arguments: &[Vec<u8>],
  expanding: &[&[u8]],
) -> Result<Vec<u8>, &'static str> {
  let expanded = pieces.iter().map(|piece| {
    return match piece {
      Segment::Text(text) => Ok(text.clone()),
      Segment::Param(index) => {
        let argument = arguments.get(*index).cloned();
        argument.ok_or("a type alias parameter has no argument")
      }
      Segment::Alias { name, arguments: written } => {
        expand_alias(table, name, written, arguments, expanding)
      }
    };
  });

  let parts = expanded.collect::<Result<Vec<_>, _>>()?;
  return Ok(parts.concat());
}

fn expand_alias(
  table: &AliasTable,
  name: &[u8],
  written: &[Vec<Segment>],
  outer_arguments: &[Vec<u8>],
  expanding: &[&[u8]],
) -> Result<Vec<u8>, &'static str> {
  let key = name.to_ascii_lowercase();
  let is_cycle = expanding.contains(&key.as_slice());
  if is_cycle {
    return Err("a type alias refers to itself");
  }

  let declared = table.definitions.get(&key);
  let definition = declared.ok_or("this type alias is not declared in the project")?;
  let parameter_count = definition.defaults.len();
  let has_too_many = written.len() > parameter_count;
  if has_too_many {
    return Err("too many type arguments for this type alias");
  }

  let given = written
    .iter()
    .map(|argument| {
      let expanded = expand(table, argument, outer_arguments, expanding)?;
      return Ok(grouped(expanded));
    })
    .collect::<Result<Vec<_>, &'static str>>()?;

  let nested = [expanding, &[key.as_slice()]].concat();
  let missing = definition.defaults.iter().skip(given.len());
  let arguments = missing.into_iter().try_fold(given, |filled, default| {
    let default = default.as_ref().ok_or("missing type arguments for this type alias")?;
    let expanded = expand(table, default, &filled, &nested)?;
    let value = grouped(expanded);
    return Ok::<_, &'static str>([filled, vec![value]].concat());
  })?;

  let body = expand(table, &definition.body, &arguments, &nested)?;
  return Ok([b"(", body.as_slice(), b")"].concat());
}

pub fn type_aliases(source: &[u8]) -> Result<Vec<TypeAlias>, StripError> {
  guard_size(source)?;
  let lexed = lex(source)?;
  let sites = find_sites(source, &lexed.tokens)?;
  let aliases = sites.alias_declarations.iter().map(|declaration| {
    let declared_at = declaration.removal.start;
    let namespace = namespace_at(&sites.names, declared_at);
    let classify = |reference: ByteSpan, written: &[u8]| {
      let param = declaration.params.iter().position(|param| param.name == written);
      if let Some(index) = param {
        return Segment::Param(index);
      }

      return match alias_name(&sites, reference.start, written) {
        Some(alias) => Segment::Alias { name: alias, arguments: Vec::new() },
        None => Segment::Text(resolve_class(&sites.names, reference.start, written)),
      };
    };

    let pieces_of = |span: ByteSpan| {
      let references = type_references(source, span);
      let references = references.map_err(|reason| StripError::at(source, span.start, reason))?;
      return Ok::<_, StripError>(segments(source, span, &references, &classify));
    };

    let body = pieces_of(declaration.body)?;
    let defaults = declaration
      .params
      .iter()
      .map(|param| param.default.map(pieces_of).transpose())
      .collect::<Result<Vec<_>, _>>()?;

    let name = qualify(namespace, &declaration.name);
    let line = line_at(source, declared_at);
    let definition = AliasDefinition { defaults, body };
    return Ok(TypeAlias { name, line, definition });
  });

  return aliases.collect();
}

pub fn alias_table<'a>(aliases: impl IntoIterator<Item = &'a TypeAlias>) -> AliasTable {
  let entries = aliases.into_iter().map(|alias| {
    let key = alias.name.to_ascii_lowercase();
    return (key, alias.definition.clone());
  });

  return AliasTable { definitions: entries.collect() };
}
