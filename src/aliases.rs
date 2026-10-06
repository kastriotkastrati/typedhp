mod structure;

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
use crate::types::reference_spans;
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

pub fn segments(
  source: &[u8],
  span: ByteSpan,
  references: &[ByteSpan],
  classify: impl Fn(ByteSpan, &[u8]) -> Segment,
) -> Vec<Segment> {
  let text_starts =
    std::iter::once(span.start).chain(references.iter().map(|reference| reference.end));

  let text_ends =
    references.iter().map(|reference| reference.start).chain(std::iter::once(span.end));

  let texts =
    text_starts.zip(text_ends).map(|(start, end)| Segment::Text(source[start..end].to_vec()));

  let classified = references.iter().map(|reference| {
    let written = &source[reference.start..reference.end];
    return classify(*reference, written);
  });

  return texts.interleave(classified).collect();
}

pub fn expand(
  table: &AliasTable,
  pieces: &[Segment],
  expanding: &[&[u8]],
) -> Result<Vec<u8>, &'static str> {
  let expanded = pieces.iter().map(|piece| {
    return match piece {
      Segment::Text(text) => Ok(text.clone()),
      Segment::Alias(name) => {
        let key = name.to_ascii_lowercase();
        let is_cycle = expanding.contains(&key.as_slice());
        if is_cycle {
          return Err("a type alias refers to itself");
        }

        let body =
          table.definitions.get(&key).ok_or("this type alias is not declared in the project")?;

        let nested = [expanding, &[key.as_slice()]].concat();
        let inner = expand(table, body, &nested)?;
        Ok([b"(", inner.as_slice(), b")"].concat())
      }
    };
  });

  return expanded.collect::<Result<Vec<_>, _>>().map(|parts| parts.concat());
}

pub fn type_aliases(source: &[u8]) -> Result<Vec<TypeAlias>, StripError> {
  guard_size(source)?;
  let lexed = lex(source)?;
  let sites = find_sites(source, &lexed.tokens)?;
  let aliases = sites.alias_declarations.iter().map(|declaration| {
    let declared_at = declaration.removal.start;
    let namespace = namespace_at(&sites.names, declared_at);
    let references = reference_spans(source, declaration.body)
      .map_err(|reason| StripError::at(source, declaration.body.start, reason))?;

    let classify = |reference: ByteSpan, written: &[u8]| {
      return match alias_name(&sites, reference.start, written) {
        Some(alias) => Segment::Alias(alias),
        None => Segment::Text(resolve_class(&sites.names, reference.start, written)),
      };
    };

    let pieces = segments(source, declaration.body, &references, classify);
    let name = qualify(namespace, &declaration.name);
    return Ok(TypeAlias { name, line: line_at(source, declared_at), segments: pieces });
  });

  return aliases.collect();
}

pub fn alias_table<'a>(aliases: impl IntoIterator<Item = &'a TypeAlias>) -> AliasTable {
  let entries =
    aliases.into_iter().map(|alias| (alias.name.to_ascii_lowercase(), alias.segments.clone()));

  return AliasTable { definitions: entries.collect() };
}
