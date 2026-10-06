mod structure;

pub use structure::Declarations;
pub use structure::Erased;
pub use structure::Erasure;
pub use structure::Member;
pub use structure::Scope;
pub use structure::TypeParam;
pub use structure::TypeParamList;
pub use structure::TypeParamUse;
pub use structure::Variance;

use crate::units::ByteSpan;
use crate::units::to_offset;
use crate::units::to_position;
use itertools::Itertools;
use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_phpdoc_syntax::cst::r#type::CallableTypeKind;
use mago_phpdoc_syntax::cst::r#type::GenericParameters;
use mago_phpdoc_syntax::cst::r#type::ReferenceKind;
use mago_phpdoc_syntax::cst::r#type::Type;
use mago_phpdoc_syntax::parse_type;
use mago_span::HasSpan;
use mago_span::Position;
use mago_span::Span;
use structure::EraseContext;

fn source_span(start: usize, end: usize) -> Result<Span, &'static str> {
  let start_position = Position::new(to_position(start)?);
  let end_position = Position::new(to_position(end)?);
  return Ok(Span::new(FileId::zero(), start_position, end_position));
}

fn byte_span(span: Span) -> Result<ByteSpan, &'static str> {
  let start = to_offset(span.start.offset)?;
  let end = to_offset(span.end.offset)?;
  return Ok(ByteSpan { start, end });
}

fn skip_whitespace(source: &[u8], start: usize) -> usize {
  let rest = &source[start.min(source.len())..];
  let whitespace = rest.iter().take_while(|byte| byte.is_ascii_whitespace()).count();
  return start + whitespace;
}

pub fn parse_span(
  source: &[u8],
  start: usize,
  limit: usize,
) -> Result<Option<ByteSpan>, &'static str> {
  let Some(content) = source.get(start..limit) else {
    return Ok(None);
  };

  let arena = LocalArena::new();
  let parsed = parse_type(&arena, content, source_span(start, limit)?);
  let Ok(parsed_type) = parsed else {
    return Ok(None);
  };

  return byte_span(parsed_type.span()).map(Some);
}

pub fn generic_arguments_span(
  source: &[u8],
  less_than: usize,
  limit: usize,
) -> Result<ByteSpan, &'static str> {
  let invalid = "invalid generic arguments";
  let anchor = less_than.checked_sub(1).ok_or(invalid)?;
  let arguments = source.get(less_than..limit).ok_or(invalid)?;
  let content = [b"X".as_slice(), arguments].concat();
  let arena = LocalArena::new();
  let parsed = parse_type(&arena, &content, source_span(anchor, limit)?).map_err(|_| invalid)?;
  let Type::Reference(reference) = &parsed else {
    return Err(invalid);
  };

  let Some(parameters) = &reference.parameters else {
    return Err(invalid);
  };

  let end = to_offset(parameters.greater_than.end.offset)?;
  return Ok(ByteSpan { start: less_than, end });
}

fn clause(source: &[u8], at: usize, marker: u8) -> Result<Option<ByteSpan>, &'static str> {
  let has_marker = source.get(at) == Some(&marker);
  if !has_marker {
    return Ok(None);
  }

  let type_start = skip_whitespace(source, at + 1);
  let Some(span) = parse_span(source, type_start, source.len())? else {
    return Err("invalid type in a type parameter list");
  };

  return Ok(Some(span));
}

fn after_clause(source: &[u8], clause: Option<ByteSpan>, fallback: usize) -> usize {
  return match clause {
    Some(span) => skip_whitespace(source, span.end),
    None => fallback,
  };
}

fn next_type_param(source: &[u8], position: usize) -> Result<(TypeParam, usize), &'static str> {
  let starts_identifier = |byte: &u8| byte.is_ascii_alphabetic() || *byte == b'_' || *byte >= 0x80;
  let at_marker = skip_whitespace(source, position);
  let variance = match source.get(at_marker) {
    Some(b'+') => Variance::Covariant,
    Some(b'-') => Variance::Contravariant,
    _ => Variance::Invariant,
  };

  let has_variance = variance != Variance::Invariant;
  let name_start = if has_variance { skip_whitespace(source, at_marker + 1) } else { at_marker };
  let Some(name_tail) = source.get(name_start..) else {
    return Err("unclosed type parameter list");
  };

  let has_name = name_tail.first().is_some_and(starts_identifier);
  if !has_name {
    return Err("expected a type parameter name");
  }

  let name_length =
    name_tail.iter().take_while(|byte| starts_identifier(byte) || byte.is_ascii_digit()).count();

  let name_end = name_start + name_length;
  let after_name = skip_whitespace(source, name_end);
  let bound = clause(source, after_name, b':')?;
  let after_bound = after_clause(source, bound, after_name);
  let default_type = clause(source, after_bound, b'=')?;
  let after_default = after_clause(source, default_type, after_bound);
  let closing = match source.get(after_default) {
    Some(b',') => skip_whitespace(source, after_default + 1),
    Some(b'>') => after_default,
    _ => return Err("expected `,` or `>` in a type parameter list"),
  };

  let name = source[name_start..name_end].to_vec();
  return Ok((TypeParam { name, bound, variance }, closing));
}

pub fn type_params(source: &[u8], less_than: usize) -> Result<TypeParamList, &'static str> {
  let first = next_type_param(source, less_than + 1);
  let parsed = std::iter::successors(Some(first), |previous| {
    let Ok((_, closing)) = previous else {
      return None;
    };

    let is_closed = source.get(*closing) == Some(&b'>');
    if is_closed {
      return None;
    }

    return Some(next_type_param(source, *closing));
  })
  .collect::<Result<Vec<_>, _>>()?;

  let closing =
    parsed.last().map(|(_, closing)| *closing).ok_or("expected a type parameter name")?;

  let params = parsed.into_iter().map(|(param, _)| param).collect();
  return Ok(TypeParamList { span: ByteSpan { start: less_than, end: closing + 1 }, params });
}

fn named(name: &[u8], extended: bool) -> Erasure {
  return Erasure { erased: Erased::Members(vec![Member::Named(name.to_vec())]), extended };
}

fn mixed(extended: bool) -> Erasure {
  return Erasure { erased: Erased::Mixed, extended };
}

fn native(written: &[u8], native_name: &[u8], has_extras: bool) -> Erasure {
  let is_native_spelling = written.eq_ignore_ascii_case(native_name);
  let keeps_spelling = is_native_spelling && !has_extras;
  if keeps_spelling {
    return named(written, false);
  }

  return named(native_name, true);
}

fn members(names: &[&str]) -> Erasure {
  let listed = names.iter().map(|name| Member::Named(name.as_bytes().to_vec()));
  return Erasure { erased: Erased::Members(listed.collect()), extended: true };
}

fn union(left: Erasure, right: Erasure) -> Erasure {
  let extended = left.extended || right.extended;
  let (Erased::Members(left_members), Erased::Members(right_members)) = (left.erased, right.erased)
  else {
    return mixed(extended);
  };

  let combined = left_members.into_iter().chain(right_members);
  let unique = combined.unique_by(|member| {
    return match member {
      Member::Named(name) => name.to_ascii_lowercase(),
      Member::Intersection(parts) => {
        let lowered = parts.iter().map(|part| part.to_ascii_lowercase()).collect::<Vec<_>>();
        lowered.join(b"&".as_slice())
      }
    };
  });

  return Erasure { erased: Erased::Members(unique.collect()), extended };
}

fn intersection_parts(members: Vec<Member>) -> Result<Vec<Vec<u8>>, &'static str> {
  let [member] =
    <[Member; 1]>::try_from(members).map_err(|_| "an intersection cannot contain a union")?;

  return match member {
    Member::Named(name) => Ok(vec![name]),
    Member::Intersection(parts) => Ok(parts),
  };
}

fn intersection(left: Erasure, right: Erasure) -> Result<Erasure, &'static str> {
  let extended = left.extended || right.extended;
  let erased = match (left.erased, right.erased) {
    (Erased::Mixed, other) | (other, Erased::Mixed) => other,
    (Erased::Members(left_members), Erased::Members(right_members)) => {
      let parts = [intersection_parts(left_members)?, intersection_parts(right_members)?].concat();
      Erased::Members(vec![Member::Intersection(parts)])
    }
  };

  return Ok(Erasure { erased, extended });
}

pub fn type_param_at<'a>(
  declarations: &'a Declarations,
  position: usize,
  name: &[u8],
) -> Option<&'a TypeParam> {
  let enclosing = declarations.scopes.iter().filter(|scope| scope.span.contains(position));
  let declared = enclosing.filter_map(|scope| {
    let type_param = scope.params.iter().find(|param| param.name.as_slice() == name)?;
    return Some((scope.span.start, type_param));
  });

  let innermost = declared.max_by_key(|(start, _)| *start);
  return innermost.map(|(_, type_param)| type_param);
}

pub fn reference_spans(source: &[u8], span: ByteSpan) -> Result<Vec<ByteSpan>, &'static str> {
  let content = source.get(span.start..span.end).ok_or("type outside the source")?;
  let arena = LocalArena::new();
  let parsed =
    parse_type(&arena, content, source_span(span.start, span.end)?).map_err(|_| "invalid type")?;

  return references(&parsed).into_iter().map(byte_span).collect();
}

pub fn argument_reference_spans(
  source: &[u8],
  arguments: ByteSpan,
) -> Result<Vec<ByteSpan>, &'static str> {
  let invalid = "invalid generic arguments";
  let anchor = arguments.start.checked_sub(1).ok_or(invalid)?;
  let written = source.get(arguments.start..arguments.end).ok_or(invalid)?;
  let content = [b"X".as_slice(), written].concat();
  let arena = LocalArena::new();
  let parsed =
    parse_type(&arena, &content, source_span(anchor, arguments.end)?).map_err(|_| invalid)?;

  let spans = references(&parsed).into_iter().map(byte_span).collect::<Result<Vec<_>, _>>()?;
  return Ok(spans.into_iter().filter(|span| span.start >= arguments.start).collect());
}

fn references(node: &Type<'_>) -> Vec<Span> {
  let own = match node {
    Type::Reference(reference) => identifier_span(&reference.kind),
    Type::MemberReference(member) => identifier_span(&member.kind),
    _ => None,
  };

  let nested = children(node).into_iter().flat_map(references);
  return own.into_iter().chain(nested).collect();
}

fn identifier_span(kind: &ReferenceKind<'_>) -> Option<Span> {
  return match kind {
    ReferenceKind::Identifier(identifier) => Some(identifier.span),
    ReferenceKind::Static(_) | ReferenceKind::Parent(_) | ReferenceKind::Self_(_) => None,
  };
}

fn entries<'node, 'arena>(
  parameters: Option<&'node GenericParameters<'arena>>,
) -> Vec<&'node Type<'arena>> {
  return parameters
    .into_iter()
    .flat_map(|list| list.entries.iter())
    .map(|entry| &entry.inner)
    .collect();
}

fn children<'node, 'arena>(node: &'node Type<'arena>) -> Vec<&'node Type<'arena>> {
  return match node {
    Type::Parenthesized(parenthesized) => vec![parenthesized.inner],
    Type::Union(union_type) => vec![union_type.left, union_type.right],
    Type::Intersection(intersection_type) => vec![intersection_type.left, intersection_type.right],
    Type::Nullable(nullable) => vec![nullable.inner],
    Type::TrailingPipe(trailing) => vec![trailing.inner],
    Type::Negated(negated) => vec![negated.operand],
    Type::Posited(posited) => vec![posited.operand],
    Type::Slice(slice) => vec![slice.inner],
    Type::IndexAccess(access) => vec![access.target, access.index],
    Type::Conditional(conditional) => {
      vec![conditional.subject, conditional.target, conditional.then, conditional.r#else]
    }
    Type::Array(array) => entries(array.parameters.as_ref()),
    Type::NonEmptyArray(array) => entries(array.parameters.as_ref()),
    Type::AssociativeArray(array) => entries(array.parameters.as_ref()),
    Type::List(list) => entries(list.parameters.as_ref()),
    Type::NonEmptyList(list) => entries(list.parameters.as_ref()),
    Type::Iterable(iterable) => entries(iterable.parameters.as_ref()),
    Type::Reference(reference) => entries(reference.parameters.as_ref()),
    Type::IntMask(mask) => entries(Some(&mask.parameters)),
    Type::TemplateType(template) => entries(Some(&template.parameters)),
    Type::ClassString(string) => {
      string.parameter.iter().map(|parameter| &parameter.entry.inner).collect()
    }
    Type::ClassLikeString(string) => {
      string.parameter.iter().map(|parameter| &parameter.entry.inner).collect()
    }
    Type::InterfaceString(string) => {
      string.parameter.iter().map(|parameter| &parameter.entry.inner).collect()
    }
    Type::EnumString(string) => {
      string.parameter.iter().map(|parameter| &parameter.entry.inner).collect()
    }
    Type::TraitString(string) => {
      string.parameter.iter().map(|parameter| &parameter.entry.inner).collect()
    }
    Type::KeyOf(key_of) => vec![&key_of.parameter.entry.inner],
    Type::ValueOf(value_of) => vec![&value_of.parameter.entry.inner],
    Type::New(new_type) => vec![&new_type.parameter.entry.inner],
    Type::PropertiesOf(properties) => vec![&properties.parameter.entry.inner],
    Type::IntMaskOf(mask) => vec![&mask.parameter.entry.inner],
    Type::Object(object) => object
      .properties
      .iter()
      .flat_map(|properties| properties.fields.iter())
      .map(|field| field.value)
      .collect(),
    Type::Shape(shape) => {
      let values = shape.fields.iter().map(|field| field.value);
      let additional =
        shape.additional_fields.iter().flat_map(|fields| entries(fields.parameters.as_ref()));

      values.chain(additional).collect()
    }
    Type::Callable(callable) => {
      let specification = callable.specification.iter();
      let parameters = specification
        .clone()
        .flat_map(|specification| specification.parameters.entries.iter())
        .filter_map(|parameter| parameter.parameter_type.as_ref());

      let returns = specification.filter_map(|specification| specification.return_type.as_ref());
      parameters.chain(returns.map(|returned| returned.return_type)).collect()
    }
    _ => Vec::new(),
  };
}

pub fn erase(
  source: &[u8],
  span: ByteSpan,
  declarations: &Declarations,
  type_param_use: TypeParamUse,
) -> Result<Erasure, &'static str> {
  let content = source.get(span.start..span.end).ok_or("type outside the source")?;
  let arena = LocalArena::new();
  let parsed =
    parse_type(&arena, content, source_span(span.start, span.end)?).map_err(|_| "invalid type")?;

  let parsed_end = to_offset(parsed.span().end.offset)?;
  let is_complete = parsed_end == span.end;
  if !is_complete {
    return Err("invalid type");
  }

  let context = EraseContext { source, declarations, position: span.start, type_param_use };
  return erase_type(&parsed, &context);
}

fn erase_type(node: &Type<'_>, context: &EraseContext<'_>) -> Result<Erasure, &'static str> {
  return match node {
    Type::Parenthesized(parenthesized) => erase_type(parenthesized.inner, context),
    Type::Union(union_type) => {
      let left = erase_type(union_type.left, context)?;
      let right = erase_type(union_type.right, context)?;
      Ok(union(left, right))
    }
    Type::Intersection(intersection_type) => {
      let left = erase_type(intersection_type.left, context)?;
      let right = erase_type(intersection_type.right, context)?;
      intersection(left, right)
    }
    Type::Nullable(nullable) => {
      let inner = erase_type(nullable.inner, context)?;
      Ok(union(inner, named(b"null", false)))
    }
    Type::Negated(negated) => {
      let operand = erase_type(negated.operand, context)?;
      Ok(Erasure { erased: operand.erased, extended: true })
    }
    Type::Posited(posited) => {
      let operand = erase_type(posited.operand, context)?;
      Ok(Erasure { erased: operand.erased, extended: true })
    }
    Type::Reference(reference) => {
      let has_parameters = reference.parameters.is_some();
      let name = match &reference.kind {
        ReferenceKind::Identifier(identifier) => identifier.value,
        ReferenceKind::Self_(keyword)
        | ReferenceKind::Static(keyword)
        | ReferenceKind::Parent(keyword) => {
          return Ok(named(keyword.value, has_parameters));
        }
      };

      let type_param = type_param_at(context.declarations, context.position, name);
      let is_alias = context.declarations.aliases.iter().any(|alias| alias.as_slice() == name);
      let Some(type_param) = type_param else {
        return Ok(if is_alias { mixed(true) } else { named(name, has_parameters) });
      };

      let is_inside_bound = context.type_param_use == TypeParamUse::Mixed;
      if is_inside_bound {
        return Ok(mixed(true));
      }

      let Some(bound) = type_param.bound else {
        return Ok(mixed(true));
      };

      let bound_erasure = erase(context.source, bound, context.declarations, TypeParamUse::Mixed)?;
      Ok(Erasure { erased: bound_erasure.erased, extended: true })
    }
    Type::Callable(callable) => {
      let has_signature = callable.specification.is_some();
      let erasure = match callable.kind {
        CallableTypeKind::Callable => native(callable.keyword.value, b"callable", has_signature),
        CallableTypeKind::Closure => named(callable.keyword.value, has_signature),
        CallableTypeKind::PureCallable => named(b"callable", true),
        CallableTypeKind::PureClosure => named(b"\\Closure", true),
      };

      Ok(erasure)
    }
    Type::Mixed(_) => Ok(mixed(false)),
    Type::Null(keyword) => Ok(native(keyword.value, b"null", false)),
    Type::Void(keyword) => Ok(native(keyword.value, b"void", false)),
    Type::Never(keyword) => Ok(native(keyword.value, b"never", false)),
    Type::True(keyword) => Ok(native(keyword.value, b"true", false)),
    Type::False(keyword) => Ok(native(keyword.value, b"false", false)),
    Type::Bool(keyword) => Ok(native(keyword.value, b"bool", false)),
    Type::Float(keyword) => Ok(native(keyword.value, b"float", false)),
    Type::Int(keyword) => Ok(native(keyword.value, b"int", false)),
    Type::String(keyword) => Ok(native(keyword.value, b"string", false)),
    Type::Array(array) => Ok(native(array.keyword.value, b"array", array.parameters.is_some())),
    Type::Iterable(iterable) => {
      Ok(native(iterable.keyword.value, b"iterable", iterable.parameters.is_some()))
    }
    Type::Object(object) => {
      Ok(native(object.keyword.value, b"object", object.properties.is_some()))
    }
    Type::NonEmptyArray(_)
    | Type::AssociativeArray(_)
    | Type::List(_)
    | Type::NonEmptyList(_)
    | Type::Shape(_) => Ok(named(b"array", true)),
    Type::PositiveInt(_)
    | Type::NegativeInt(_)
    | Type::NonPositiveInt(_)
    | Type::NonNegativeInt(_)
    | Type::NonZeroInt(_)
    | Type::UnspecifiedLiteralInt(_)
    | Type::LiteralInt(_)
    | Type::IntRange(_)
    | Type::IntMask(_)
    | Type::IntMaskOf(_) => Ok(named(b"int", true)),
    Type::UnspecifiedLiteralFloat(_) | Type::LiteralFloat(_) => Ok(named(b"float", true)),
    Type::ClassString(_)
    | Type::ClassLikeString(_)
    | Type::InterfaceString(_)
    | Type::EnumString(_)
    | Type::TraitString(_)
    | Type::CallableString(_)
    | Type::LowercaseCallableString(_)
    | Type::UppercaseCallableString(_)
    | Type::NumericString(_)
    | Type::NonEmptyString(_)
    | Type::NonEmptyLowercaseString(_)
    | Type::LowercaseString(_)
    | Type::NonEmptyUppercaseString(_)
    | Type::UppercaseString(_)
    | Type::TruthyString(_)
    | Type::NonFalsyString(_)
    | Type::UnspecifiedLiteralString(_)
    | Type::NonEmptyUnspecifiedLiteralString(_)
    | Type::LiteralString(_) => Ok(named(b"string", true)),
    Type::ArrayKey(_) => Ok(members(&["int", "string"])),
    Type::Numeric(_) => Ok(members(&["int", "float", "string"])),
    Type::Scalar(_) => Ok(members(&["int", "float", "string", "bool"])),
    Type::StringableObject(_) => Ok(named(b"object", true)),
    Type::ThisVariable(_) => Ok(named(b"static", true)),
    Type::TrailingPipe(_) => Err("incomplete union type"),
    _ => Ok(mixed(true)),
  };
}

pub fn render(erased: &Erased) -> Vec<u8> {
  let Erased::Members(members) = erased else {
    return b"mixed".to_vec();
  };

  let non_null = members
    .iter()
    .filter(|member| !matches!(member, Member::Named(name) if name.eq_ignore_ascii_case(b"null")))
    .collect::<Vec<_>>();

  let has_null = non_null.len() < members.len();
  if has_null && let [Member::Named(name)] = non_null.as_slice() {
    return [b"?".as_slice(), name.as_slice()].concat();
  }

  let needs_parentheses = members.len() > 1;
  let rendered = members.iter().map(|member| {
    return match member {
      Member::Named(name) => name.clone(),
      Member::Intersection(parts) => {
        let joined = parts.join(b"&".as_slice());
        if needs_parentheses {
          [b"(".as_slice(), joined.as_slice(), b")".as_slice()].concat()
        } else {
          joined
        }
      }
    };
  });

  return rendered.collect::<Vec<_>>().join(b"|".as_slice());
}
