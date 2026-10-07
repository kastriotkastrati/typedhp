mod structure;

pub use structure::Edit;
pub use structure::StripError;

use crate::lex::lex;
use crate::sites::Sites;
use crate::sites::find_sites;
use crate::types::TypeParamUse;
use crate::types::erase;
use crate::types::render;
use crate::units::to_position;
use itertools::Itertools;

pub fn guard_size(source: &[u8]) -> Result<(), StripError> {
  let fits = to_position(source.len()).is_ok();
  if !fits {
    return Err(StripError { line: 1, reason: "source file is larger than 4 GiB" });
  }

  return Ok(());
}

pub fn strip_edits(source: &[u8], sites: &Sites) -> Result<Vec<Edit>, StripError> {
  let removals = sites.removals.iter().map(|span| Edit { span: *span, replacement: Vec::new() });
  let erasures = sites
    .type_spans()
    .iter()
    .map(|span| {
      let erasure = erase(source, *span, &sites.declarations, TypeParamUse::Resolve);
      let erasure = erasure.map_err(|reason| StripError::at(source, span.start, reason))?;
      if !erasure.extended {
        return Ok(None);
      }

      return Ok(Some(Edit { span: *span, replacement: render(&erasure.erased) }));
    })
    .collect::<Result<Vec<_>, StripError>>()?;

  return Ok(removals.chain(erasures.into_iter().flatten()).collect());
}

pub fn splice(source: &[u8], edits: &[Edit]) -> Result<Vec<u8>, StripError> {
  let ordered =
    edits.iter().sorted_by_key(|edit| (edit.span.start, edit.span.end)).collect::<Vec<_>>();

  let overlap =
    ordered.iter().tuple_windows().find(|(earlier, later)| later.span.start < earlier.span.end);

  if let Some((_, later)) = overlap {
    return Err(StripError::at(source, later.span.start, "two rewrites overlap"));
  }

  let kept_starts = std::iter::once(0).chain(ordered.iter().map(|edit| edit.span.end));
  let kept_ends = ordered.iter().map(|edit| edit.span.start).chain(std::iter::once(source.len()));
  let kept = kept_starts.zip(kept_ends).map(|(start, end)| &source[start..end]);
  let replaced = ordered.iter().map(|edit| edit.replacement.as_slice());
  let pieces = kept.interleave(replaced).collect::<Vec<&[u8]>>();
  return Ok(pieces.concat());
}

pub fn apply_edits(source: &[u8], edits: &[Edit]) -> Result<Vec<u8>, StripError> {
  let line_keeping = edits.iter().map(|edit| {
    let removed = &source[edit.span.start..edit.span.end];
    let newlines = removed.iter().filter(|byte| **byte == b'\n').count();
    let kept_lines = vec![b'\n'; newlines];
    let replacement = [edit.replacement.as_slice(), &kept_lines].concat();
    return Edit { span: edit.span, replacement };
  });

  return splice(source, &line_keeping.collect::<Vec<_>>());
}

pub fn strip(source: &[u8]) -> Result<Vec<u8>, StripError> {
  guard_size(source)?;
  let lexed = lex(source)?;
  let sites = find_sites(source, &lexed.tokens)?;
  let edits = strip_edits(source, &sites)?;
  return apply_edits(source, &edits);
}
