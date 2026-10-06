mod structure;

pub use structure::Desugared;

use crate::aliases::AliasTable;
use crate::aliases::Segment;
use crate::aliases::alias_name;
use crate::aliases::expand;
use crate::aliases::segments;
use crate::lex::lex;
use crate::sites::find_sites;
use crate::strip::Edit;
use crate::strip::StripError;
use crate::strip::apply_edits;
use crate::strip::guard_size;
use crate::strip::strip_edits;
use crate::types::TypeParam;
use crate::types::TypeParamUse;
use crate::types::Variance;
use crate::types::argument_reference_spans;
use crate::types::erase;
use crate::types::reference_spans;
use crate::types::type_param_at;
use crate::units::ByteSpan;
use itertools::Itertools;
use structure::Annotation;
use structure::TagContext;

fn tag_text(
  context: &TagContext<'_>,
  span: ByteSpan,
  references: &[ByteSpan],
) -> Result<Vec<u8>, StripError> {
  let classify = |reference: ByteSpan, written: &[u8]| {
    let is_type_param =
      type_param_at(&context.sites.declarations, reference.start, written).is_some();

    let alias =
      if is_type_param { None } else { alias_name(context.sites, reference.start, written) };

    return match alias {
      Some(name) => Segment::Alias(name),
      None => Segment::Text(written.to_vec()),
    };
  };

  let pieces = segments(context.source, span, references, classify);
  let expanded = expand(context.table, &pieces, &[])
    .map_err(|reason| StripError::at(context.source, span.start, reason))?;

  let one_line =
    expanded.iter().map(|byte| if matches!(byte, b'\n' | b'\r') { b' ' } else { *byte });

  return Ok(one_line.collect());
}

fn tag_type(context: &TagContext<'_>, span: ByteSpan) -> Result<Vec<u8>, StripError> {
  let references = reference_spans(context.source, span)
    .map_err(|reason| StripError::at(context.source, span.start, reason))?;

  return tag_text(context, span, &references);
}

fn is_extended(context: &TagContext<'_>, span: ByteSpan) -> Result<bool, StripError> {
  let erasure = erase(context.source, span, &context.sites.declarations, TypeParamUse::Resolve);
  let erasure = erasure.map_err(|reason| StripError::at(context.source, span.start, reason))?;
  return Ok(erasure.extended);
}

fn template_tags(
  context: &TagContext<'_>,
  params: &[TypeParam],
) -> Result<Vec<Vec<u8>>, StripError> {
  let tags = params.iter().map(|param| {
    let variance: &[u8] = match param.variance {
      Variance::Invariant => b"",
      Variance::Covariant => b"-covariant",
      Variance::Contravariant => b"-contravariant",
    };

    let head = [b"@template", variance, b" ", param.name.as_slice()].concat();
    let Some(bound) = param.bound else {
      return Ok(head);
    };

    let bound_text = tag_type(context, bound)?;
    return Ok([head.as_slice(), b" of ", bound_text.as_slice()].concat());
  });

  return tags.collect();
}

fn extended_tag(
  context: &TagContext<'_>,
  tag: &[u8],
  span: ByteSpan,
  subject: &[u8],
) -> Result<Option<Vec<u8>>, StripError> {
  let extended = is_extended(context, span)?;
  if !extended {
    return Ok(None);
  }

  let type_text = tag_type(context, span)?;
  let has_subject = !subject.is_empty();
  let separator: &[u8] = if has_subject { b" " } else { b"" };
  return Ok(Some([tag, b" ", type_text.as_slice(), separator, subject].concat()));
}

fn annotations(context: &TagContext<'_>) -> Result<Vec<Annotation>, StripError> {
  let sites = context.sites;
  let functions = sites.functions.iter().map(|function| {
    let templates = template_tags(context, &function.type_params)?;
    let parameters = function
      .parameters
      .iter()
      .map(|parameter| extended_tag(context, b"@param", parameter.type_span, &parameter.variable))
      .collect::<Result<Vec<_>, StripError>>()?;

    let returns =
      function.return_type.map(|span| extended_tag(context, b"@return", span, b"")).transpose()?;

    let tags =
      templates.into_iter().chain(parameters.into_iter().flatten()).chain(returns.flatten());

    return Ok(Annotation { anchor: function.anchor, tags: tags.collect() });
  });

  let classes = sites.classes.iter().map(|class| {
    let templates = template_tags(context, &class.type_params)?;
    let extends = class
      .extends
      .iter()
      .map(|span| Ok([b"@extends ", tag_type(context, *span)?.as_slice()].concat()));

    let implements = class
      .implements
      .iter()
      .map(|span| Ok([b"@implements ", tag_type(context, *span)?.as_slice()].concat()));

    let parents = extends.chain(implements).collect::<Result<Vec<_>, StripError>>()?;
    return Ok(Annotation {
      anchor: class.anchor,
      tags: templates.into_iter().chain(parents).collect(),
    });
  });

  let trait_uses = sites.trait_uses.iter().map(|trait_use| {
    let tags = trait_use
      .traits
      .iter()
      .map(|span| Ok([b"@use ", tag_type(context, *span)?.as_slice()].concat()));

    return Ok(Annotation {
      anchor: trait_use.anchor,
      tags: tags.collect::<Result<Vec<_>, StripError>>()?,
    });
  });

  let members = sites.members.iter().map(|member| {
    let tag = extended_tag(context, b"@var", member.type_span, b"")?;
    return Ok(Annotation { anchor: member.anchor, tags: tag.into_iter().collect() });
  });

  let constructions = sites.constructions.iter().map(|construction| {
    let references = argument_reference_spans(context.source, construction.arguments)
      .map_err(|reason| StripError::at(context.source, construction.arguments.start, reason))?;

    let arguments = tag_text(context, construction.arguments, &references)?;
    let class = &context.source[construction.class.start..construction.class.end];
    let variable = &context.source[construction.variable.start..construction.variable.end];
    let tag = [b"@var ", class, arguments.as_slice(), b" ", variable].concat();
    return Ok(Annotation { anchor: construction.anchor, tags: vec![tag] });
  });

  let all = functions.chain(classes).chain(trait_uses).chain(members).chain(constructions);
  return all.collect();
}

fn docblock_edit(source: &[u8], doc_comments: &[ByteSpan], annotation: &Annotation) -> Edit {
  let anchor = annotation.anchor;
  let line_start =
    source[..anchor].iter().rposition(|byte| *byte == b'\n').map_or(0, |newline| newline + 1);

  let before_anchor = &source[line_start..anchor];
  let starts_line = before_anchor.iter().all(u8::is_ascii_whitespace);
  let indent: &[u8] = if starts_line { before_anchor } else { b"" };
  let tag_lines =
    annotation.tags.iter().map(|tag| [b"\n", indent, b" * ", tag.as_slice()].concat()).concat();

  let previous_doc = doc_comments.iter().rfind(|doc| doc.end <= anchor);
  let attached_doc =
    previous_doc.filter(|doc| source[doc.end..anchor].iter().all(u8::is_ascii_whitespace));

  if let Some(doc) = attached_doc {
    let closing = doc.end - 2;
    let closing_line_start =
      source[..closing].iter().rposition(|byte| *byte == b'\n').map(|newline| newline + 1);

    let own_line = closing_line_start.filter(|start| {
      let is_inside_doc = *start > doc.start;
      return is_inside_doc && source[*start..closing].iter().all(u8::is_ascii_whitespace);
    });

    let Some(own_line) = own_line else {
      let replacement = [tag_lines.as_slice(), b"\n", indent, b" "].concat();
      return Edit { span: ByteSpan { start: closing, end: closing }, replacement };
    };

    let lines = annotation.tags.iter().map(|tag| [indent, b" * ", tag.as_slice(), b"\n"].concat());
    return Edit { span: ByteSpan { start: own_line, end: own_line }, replacement: lines.concat() };
  }

  let insertion = ByteSpan { start: anchor, end: anchor };
  let is_single_tag = annotation.tags.len() == 1;
  if is_single_tag {
    let replacement = [b"/** ", annotation.tags[0].as_slice(), b" */ "].concat();
    return Edit { span: insertion, replacement };
  }

  let replacement = [b"/**", tag_lines.as_slice(), b"\n", indent, b" */\n", indent].concat();
  return Edit { span: insertion, replacement };
}

fn line_map(source: &[u8], edits: &[Edit]) -> Vec<usize> {
  let newlines = source.iter().positions(|byte| *byte == b'\n').collect::<Vec<_>>();
  let source_lines = 1..=newlines.len() + 1;
  let inserted_lines = edits.iter().flat_map(|edit| {
    let line = newlines.partition_point(|newline| *newline < edit.span.start) + 1;
    let added = edit.replacement.iter().filter(|byte| **byte == b'\n').count();
    return std::iter::repeat_n(line, added);
  });

  return source_lines.chain(inserted_lines).sorted().collect();
}

fn output_span(source: &[u8], edits: &[Edit], inserted: &Edit) -> ByteSpan {
  let at = inserted.span.start;
  let earlier = edits.iter().filter(|edit| edit.span.start < at && edit.span.end <= at);
  let (added, removed) = earlier.fold((0, 0), |totals, edit| {
    let (added, removed) = totals;
    let removed_text = &source[edit.span.start..edit.span.end];
    let kept_newlines = removed_text.iter().filter(|byte| **byte == b'\n').count();
    return (added + edit.replacement.len() + kept_newlines, removed + removed_text.len());
  });

  let start = at + added - removed;
  return ByteSpan { start, end: start + inserted.replacement.len() };
}

pub fn desugar(source: &[u8], table: &AliasTable) -> Result<Desugared, StripError> {
  guard_size(source)?;
  let lexed = lex(source)?;
  let sites = find_sites(source, &lexed.tokens)?;
  let context = TagContext { source, sites: &sites, table };
  let annotated = annotations(&context)?;
  let tagged = annotated.iter().filter(|annotation| !annotation.tags.is_empty());
  let docblock_edits = tagged
    .map(|annotation| docblock_edit(source, &lexed.doc_comments, annotation))
    .collect::<Vec<_>>();

  let edits = [strip_edits(source, &sites)?, docblock_edits.clone()].concat();
  let code = apply_edits(source, &edits)?;
  let lines = line_map(source, &edits);
  let docblocks = docblock_edits.iter().map(|edit| output_span(source, &edits, edit)).collect();
  return Ok(Desugared { code, lines, docblocks });
}
