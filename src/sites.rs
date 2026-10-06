mod structure;

pub use structure::AliasDeclaration;
pub use structure::ClassHeader;
pub use structure::Code;
pub use structure::Construction;
pub use structure::FunctionHeader;
pub use structure::Member;
pub use structure::Parameter;
pub use structure::Sites;
pub use structure::TraitUse;

use crate::lex::Kind;
use crate::lex::Token;
use crate::names::ImportKind;
use crate::names::find_names;
use crate::strip::StripError;
use crate::types::Declarations;
use crate::types::Scope;
use crate::types::TypeParamList;
use crate::types::Variance;
use crate::types::generic_arguments_span;
use crate::types::parse_span;
use crate::types::type_params;
use crate::units::ByteSpan;
use itertools::Itertools;
use structure::AngleGroup;
use structure::Clause;

fn first_token_from(tokens: &[Token], offset: usize) -> usize {
  return tokens.partition_point(|token| token.span.start < offset);
}

fn kind_at(tokens: &[Token], index: usize) -> Option<Kind> {
  return tokens.get(index).map(|token| token.kind);
}

fn previous_kind(tokens: &[Token], index: usize) -> Option<Kind> {
  let previous = index.checked_sub(1)?;
  return kind_at(tokens, previous);
}

fn matching_close(tokens: &[Token], open_index: usize) -> Option<usize> {
  let open = tokens.get(open_index)?;
  let opens = open.kind.is_opener();
  if !opens {
    return None;
  }

  let after = tokens.get(open_index + 1..)?;
  let offset =
    after.iter().position(|token| token.kind.is_closer() && token.depth == open.depth)?;

  return Some(open_index + 1 + offset);
}

fn matching_open(tokens: &[Token], close_index: usize) -> Option<usize> {
  let close = tokens.get(close_index)?;
  let before = tokens.get(..close_index)?;
  return before.iter().rposition(|token| token.kind.is_opener() && token.depth == close.depth);
}

fn enclosing_opener(tokens: &[Token], index: usize) -> Option<usize> {
  let token = tokens.get(index)?;
  let parent_depth = token.depth.checked_sub(1)?;
  let before = tokens.get(..index)?;
  return before
    .iter()
    .rposition(|candidate| candidate.kind.is_opener() && candidate.depth == parent_depth);
}

fn starts_statement(previous: Option<Kind>) -> bool {
  return matches!(
    previous,
    None | Some(Kind::Semicolon | Kind::OpenBrace | Kind::CloseBrace | Kind::OpenTag)
  );
}

fn declaration_start(tokens: &[Token], keyword_index: usize) -> usize {
  let earlier_start = |start: &usize| {
    let previous = start.checked_sub(1)?;
    return match tokens[previous].kind {
      Kind::Modifier => Some(previous),
      Kind::CloseBracket => {
        matching_open(tokens, previous).filter(|open| tokens[*open].kind == Kind::OpenAttribute)
      }
      _ => None,
    };
  };

  let starts = std::iter::successors(Some(keyword_index), earlier_start);
  return starts.fold(keyword_index, |_, earliest| earliest);
}

fn statement_limit(code: &Code<'_>, from_index: usize) -> usize {
  let Some(rest) = code.tokens.get(from_index..) else {
    return code.source.len();
  };

  let boundary = rest.iter().find(|token| matches!(token.kind, Kind::Semicolon | Kind::OpenBrace));
  return match boundary {
    Some(token) => token.span.start,
    None => code.source.len(),
  };
}

fn follows_parameter_type(rest: &[u8]) -> bool {
  return rest.starts_with(b"$") || rest.starts_with(b"&") || rest.starts_with(b"...");
}

fn follows_return_type(rest: &[u8]) -> bool {
  return rest.starts_with(b"{")
    || rest.starts_with(b";")
    || rest.starts_with(b"=>")
    || rest.starts_with(b"?>");
}

fn follows_property_type(rest: &[u8]) -> bool {
  return rest.starts_with(b"$");
}

fn follows_constant_type(rest: &[u8]) -> bool {
  return rest
    .first()
    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_' || *byte >= 0x80);
}

fn type_span(
  code: &Code<'_>,
  start: usize,
  accepts: fn(&[u8]) -> bool,
) -> Result<Option<ByteSpan>, &'static str> {
  let following = &code.tokens[first_token_from(code.tokens, start + 1)..];
  let first_brace = following.iter().find(|token| token.kind == Kind::OpenBrace);
  let spaced_brace = following.iter().find(|token| {
    let is_brace = token.kind == Kind::OpenBrace;
    let preceding = token.span.start.checked_sub(1).and_then(|index| code.source.get(index));
    let after_whitespace = preceding.is_some_and(|byte| byte.is_ascii_whitespace());
    return is_brace && after_whitespace;
  });

  let limits = [spaced_brace, first_brace].map(|brace| {
    return match brace {
      Some(token) => token.span.start,
      None => code.source.len(),
    };
  });

  let accepted = limits.into_iter().map(|limit| {
    let span = parse_span(code.source, start, limit)?;
    return Ok(span.filter(|span| {
      let next_index = code.tokens.partition_point(|token| token.span.end <= span.end);
      let follower = match code.tokens.get(next_index) {
        Some(token) => &code.source[token.span.start.max(span.end)..],
        None => &[],
      };

      return accepts(follower);
    }));
  });

  return accepted.flatten_ok().next().transpose();
}

fn is_declaration_keyword(tokens: &[Token], index: usize) -> bool {
  let previous = previous_kind(tokens, index);
  return !matches!(previous, Some(Kind::ObjectOperator | Kind::DoubleColon | Kind::Function));
}

fn signature_start(tokens: &[Token], keyword_index: usize) -> usize {
  let after_keyword = keyword_index + 1;
  let returns_reference = kind_at(tokens, after_keyword) == Some(Kind::Ampersand);
  let after_ampersand = if returns_reference { after_keyword + 1 } else { after_keyword };
  let is_function_keyword = kind_at(tokens, keyword_index) == Some(Kind::Function);
  let is_anonymous =
    matches!(kind_at(tokens, after_ampersand), Some(Kind::OpenParen | Kind::LessThan));

  let has_name = is_function_keyword && !is_anonymous;
  if has_name {
    return after_ampersand + 1;
  }

  return after_ampersand;
}

fn declaration_group(
  code: &Code<'_>,
  owner: usize,
  less_than_index: usize,
) -> Result<Vec<AngleGroup>, StripError> {
  let Some(less_than) = code.tokens.get(less_than_index) else {
    return Ok(Vec::new());
  };

  let declares_type_params = less_than.kind == Kind::LessThan;
  if !declares_type_params {
    return Ok(Vec::new());
  }

  let start = less_than.span.start;
  let list =
    type_params(code.source, start).map_err(|reason| StripError::at(code.source, start, reason))?;

  return Ok(vec![AngleGroup::Declaration { owner, list }]);
}

fn arguments_after(code: &Code<'_>, before_index: usize) -> Result<ByteSpan, StripError> {
  let less_than = code.tokens[before_index + 1].span.start;
  let limit = statement_limit(code, before_index + 2);
  let arguments = generic_arguments_span(code.source, less_than, limit);
  return arguments.map_err(|reason| StripError::at(code.source, less_than, reason));
}

fn argument_groups(
  code: &Code<'_>,
  keyword_index: usize,
  clause: Clause,
) -> Result<Vec<AngleGroup>, StripError> {
  let first = Ok((keyword_index + 1, None));
  let visits = std::iter::successors(
    Some(first),
    |visit: &Result<(usize, Option<AngleGroup>), StripError>| {
      let Ok((index, _)) = visit else {
        return None;
      };

      let kind = kind_at(code.tokens, *index);
      let continues_list = matches!(kind, Some(Kind::Name | Kind::Comma));
      if !continues_list {
        return None;
      }

      let opens_arguments =
        kind == Some(Kind::Name) && kind_at(code.tokens, index + 1) == Some(Kind::LessThan);

      if !opens_arguments {
        return Some(Ok((index + 1, None)));
      }

      let group = arguments_after(code, *index).map(|arguments| {
        let named = Some(code.tokens[*index].span);
        let owner = keyword_index;
        let group = AngleGroup::Arguments { clause, owner, named, arguments, removal: arguments };
        return (first_token_from(code.tokens, arguments.end), Some(group));
      });

      return Some(group);
    },
  );

  return visits.filter_map_ok(|(_, group)| group).collect();
}

fn groups_at(code: &Code<'_>, index: usize) -> Result<Vec<AngleGroup>, StripError> {
  let token = code.tokens[index];
  let is_declaration = is_declaration_keyword(code.tokens, index);
  let follows_double_colon = previous_kind(code.tokens, index) == Some(Kind::DoubleColon);
  let names_declaration = kind_at(code.tokens, index + 1) == Some(Kind::Name);
  let opens_arguments = kind_at(code.tokens, index + 1) == Some(Kind::LessThan);
  return match token.kind {
    Kind::Function | Kind::Fn if is_declaration => {
      declaration_group(code, index, signature_start(code.tokens, index))
    }
    Kind::Class | Kind::Interface | Kind::Trait if !follows_double_colon && names_declaration => {
      declaration_group(code, index, index + 2)
    }
    Kind::DoubleColon if opens_arguments => {
      let arguments = arguments_after(code, index)?;
      let removal = ByteSpan { start: token.span.start, end: arguments.end };
      let before = index.checked_sub(1).map(|previous| code.tokens[previous]);
      let named =
        before.filter(|previous| previous.kind == Kind::Name).map(|previous| previous.span);

      let clause = Clause::Turbofish;
      Ok(vec![AngleGroup::Arguments { clause, owner: index, named, arguments, removal }])
    }
    Kind::Extends => argument_groups(code, index, Clause::Extends),
    Kind::Implements => argument_groups(code, index, Clause::Implements),
    Kind::Use => argument_groups(code, index, Clause::TraitUse),
    _ => Ok(Vec::new()),
  };
}

fn skip_attributes_and_modifiers(tokens: &[Token], start: usize) -> usize {
  let next_index = |index: &usize| {
    return match kind_at(tokens, *index) {
      Some(Kind::OpenAttribute) => matching_close(tokens, *index).map(|close| close + 1),
      Some(Kind::Modifier) => Some(index + 1),
      _ => None,
    };
  };

  let indices = std::iter::successors(Some(start), next_index);
  return indices.fold(start, |_, latest| latest);
}

fn parameter_at(
  code: &Code<'_>,
  angle_spans: &[ByteSpan],
  index: usize,
  close_index: usize,
  parameter_depth: usize,
) -> Result<(usize, Option<Parameter>), StripError> {
  let type_index = skip_attributes_and_modifiers(code.tokens, index);
  let is_untyped = matches!(
    kind_at(code.tokens, type_index),
    Some(Kind::Ampersand | Kind::Ellipsis | Kind::Variable)
  );

  let starts_type = type_index < close_index && !is_untyped;
  let (resume_index, parameter) = if starts_type {
    let start = code.tokens[type_index].span.start;
    let fail = |reason| StripError::at(code.source, start, reason);
    let span = type_span(code, start, follows_parameter_type).map_err(fail)?;
    let type_span = span.ok_or_else(|| fail("cannot read this parameter type"))?;
    let after_type = first_token_from(code.tokens, type_span.end);
    let markers = code.tokens[after_type..]
      .iter()
      .take_while(|token| matches!(token.kind, Kind::Ampersand | Kind::Ellipsis))
      .collect::<Vec<_>>();

    let is_variadic = markers.iter().any(|token| token.kind == Kind::Ellipsis);
    let variable_index = after_type + markers.len();
    let variable = code.tokens.get(variable_index).filter(|token| token.kind == Kind::Variable);
    let variable = variable.ok_or_else(|| fail("expected a parameter name after its type"))?;
    let name = &code.source[variable.span.start..variable.span.end];
    let prefix: &[u8] = if is_variadic { b"..." } else { b"" };
    (after_type, Some(Parameter { type_span, variable: [prefix, name].concat() }))
  } else {
    (type_index, None)
  };

  let separator = code.tokens.get(resume_index..close_index).and_then(|remaining| {
    return remaining.iter().position(|token| {
      let is_separator = token.kind == Kind::Comma && token.depth == parameter_depth;
      let inside_angle_group = angle_spans.iter().any(|span| span.contains(token.span.start));
      return is_separator && !inside_angle_group;
    });
  });

  let next_index = match separator {
    Some(offset) => resume_index + offset + 1,
    None => close_index,
  };

  return Ok((next_index, parameter));
}

fn parameters(
  code: &Code<'_>,
  angle_spans: &[ByteSpan],
  open_index: usize,
  close_index: usize,
) -> Result<Vec<Parameter>, StripError> {
  let parameter_depth = code.tokens[open_index].depth + 1;
  let first = Ok((open_index + 1, None));
  let visits =
    std::iter::successors(Some(first), |visit: &Result<(usize, Option<Parameter>), StripError>| {
      let Ok((index, _)) = visit else {
        return None;
      };

      let has_more = *index < close_index;
      if !has_more {
        return None;
      }

      return Some(parameter_at(code, angle_spans, *index, close_index, parameter_depth));
    });

  return visits.filter_map_ok(|(_, parameter)| parameter).collect();
}

fn function_header(
  code: &Code<'_>,
  groups: &[AngleGroup],
  angle_spans: &[ByteSpan],
  keyword_index: usize,
) -> Result<Option<FunctionHeader>, StripError> {
  let keyword = code.tokens[keyword_index];
  let fail = |reason| StripError::at(code.source, keyword.span.start, reason);
  let declaration = groups.iter().find_map(|group| {
    return match group {
      AngleGroup::Declaration { owner, list } if *owner == keyword_index => Some(list),
      _ => None,
    };
  });

  let open_index = match declaration {
    Some(list) => first_token_from(code.tokens, list.span.end),
    None => signature_start(code.tokens, keyword_index),
  };

  let opens_parameters = kind_at(code.tokens, open_index) == Some(Kind::OpenParen);
  if !opens_parameters {
    return Ok(None);
  }

  let close_index =
    matching_close(code.tokens, open_index).ok_or_else(|| fail("unclosed parameter list"))?;

  let parameters = parameters(code, angle_spans, open_index, close_index)?;
  let anchor = code.tokens[declaration_start(code.tokens, keyword_index)].span.start;
  let after_parameters = close_index + 1;
  let captures_variables = kind_at(code.tokens, after_parameters) == Some(Kind::Use);
  let after_captures = if captures_variables {
    let captures_close =
      matching_close(code.tokens, after_parameters + 1).ok_or_else(|| fail("unclosed use list"))?;

    captures_close + 1
  } else {
    after_parameters
  };

  let has_return_type = kind_at(code.tokens, after_captures) == Some(Kind::Colon);
  let return_type = if has_return_type {
    let return_token =
      code.tokens.get(after_captures + 1).ok_or_else(|| fail("missing return type"))?;

    let return_fail = |reason| StripError::at(code.source, return_token.span.start, reason);
    let span =
      type_span(code, return_token.span.start, follows_return_type).map_err(return_fail)?;

    Some(span.ok_or_else(|| return_fail("cannot read this return type"))?)
  } else {
    None
  };

  let Some(list) = declaration else {
    let type_params = Vec::new();
    return Ok(Some(FunctionHeader { anchor, type_params, parameters, return_type, scope: None }));
  };

  let body_index = match return_type {
    Some(span) => first_token_from(code.tokens, span.end),
    None => after_captures,
  };

  let body = code.tokens.get(body_index).ok_or_else(|| fail("expected a function body"))?;
  let body_end = match body.kind {
    Kind::OpenBrace => {
      let close =
        matching_close(code.tokens, body_index).ok_or_else(|| fail("unclosed function body"))?;

      code.tokens[close].span.end
    }
    Kind::Semicolon => body.span.end,
    Kind::Arrow => {
      let terminator = code.tokens[body_index + 1..].iter().find(|token| {
        let leaves_expression = token.kind.is_closer() && token.depth < keyword.depth;
        let separates =
          matches!(token.kind, Kind::Comma | Kind::Semicolon) && token.depth == keyword.depth;

        let closes_php = token.kind == Kind::CloseTag;
        return leaves_expression || separates || closes_php;
      });

      match terminator {
        Some(token) => token.span.start,
        None => code.source.len(),
      }
    }
    _ => return Err(fail("expected a function body")),
  };

  let scope = Scope {
    span: ByteSpan { start: keyword.span.start, end: body_end },
    params: list.params.clone(),
  };

  let type_params = list.params.clone();
  return Ok(Some(FunctionHeader {
    anchor,
    type_params,
    parameters,
    return_type,
    scope: Some(scope),
  }));
}

fn class_scope(code: &Code<'_>, group: &AngleGroup) -> Result<Option<Scope>, StripError> {
  let AngleGroup::Declaration { owner, list } = group else {
    return Ok(None);
  };

  let owner_token = code.tokens[*owner];
  let is_class_like = matches!(owner_token.kind, Kind::Class | Kind::Interface | Kind::Trait);
  if !is_class_like {
    return Ok(None);
  }

  let fail = |reason| StripError::at(code.source, owner_token.span.start, reason);
  let from = first_token_from(code.tokens, list.span.end);
  let body_offset = code.tokens[from..].iter().position(|token| token.kind == Kind::OpenBrace);
  let body_index = from + body_offset.ok_or_else(|| fail("expected a class body"))?;
  let close_index =
    matching_close(code.tokens, body_index).ok_or_else(|| fail("unclosed class body"))?;

  let span = ByteSpan { start: owner_token.span.start, end: code.tokens[close_index].span.end };
  return Ok(Some(Scope { span, params: list.params.clone() }));
}

fn class_headers(code: &Code<'_>, groups: &[AngleGroup]) -> Result<Vec<ClassHeader>, StripError> {
  let headers = (0..code.tokens.len()).filter_map(|index| {
    let token = code.tokens[index];
    let is_class_like =
      matches!(token.kind, Kind::Class | Kind::Interface | Kind::Trait | Kind::Enum);

    let follows_double_colon = previous_kind(code.tokens, index) == Some(Kind::DoubleColon);
    let names_declaration = kind_at(code.tokens, index + 1) == Some(Kind::Name);
    let is_declaration = is_class_like && !follows_double_colon && names_declaration;
    if !is_declaration {
      return None;
    }

    let body_offset = code.tokens[index..]
      .iter()
      .position(|candidate| candidate.kind == Kind::OpenBrace && candidate.depth == token.depth);

    let Some(body_offset) = body_offset else {
      return Some(Err(StripError::at(code.source, token.span.start, "expected a class body")));
    };

    let header = index..index + body_offset;
    let type_params = groups.iter().find_map(|group| {
      return match group {
        AngleGroup::Declaration { owner, list } if *owner == index => Some(list.params.clone()),
        _ => None,
      };
    });

    let clauses = |wanted: Clause| {
      return groups
        .iter()
        .filter_map(|group| {
          let AngleGroup::Arguments { clause, owner, named: Some(named), arguments, .. } = group
          else {
            return None;
          };

          let belongs = *clause == wanted && header.contains(owner);
          return belongs.then_some(ByteSpan { start: named.start, end: arguments.end });
        })
        .collect::<Vec<_>>();
    };

    let type_params = type_params.unwrap_or_default();
    let extends = clauses(Clause::Extends);
    let implements = clauses(Clause::Implements);
    let is_generic = !type_params.is_empty() || !extends.is_empty() || !implements.is_empty();
    if !is_generic {
      return None;
    }

    let anchor = code.tokens[declaration_start(code.tokens, index)].span.start;
    return Some(Ok(ClassHeader { anchor, type_params, extends, implements }));
  });

  return headers.collect();
}

fn trait_uses(code: &Code<'_>, groups: &[AngleGroup]) -> Vec<TraitUse> {
  let uses = groups.iter().filter_map(|group| {
    let AngleGroup::Arguments {
      clause: Clause::TraitUse,
      owner,
      named: Some(named),
      arguments,
      ..
    } = group
    else {
      return None;
    };

    return Some((*owner, ByteSpan { start: named.start, end: arguments.end }));
  });

  let grouped = uses.into_group_map();
  let owners = grouped.into_iter().sorted_by_key(|(owner, _)| *owner);
  return owners
    .map(|(owner, traits)| TraitUse { anchor: code.tokens[owner].span.start, traits })
    .collect();
}

fn constructions(code: &Code<'_>, groups: &[AngleGroup]) -> Vec<Construction> {
  let found = groups.iter().filter_map(|group| {
    let AngleGroup::Arguments {
      clause: Clause::Turbofish,
      owner,
      named: Some(class),
      arguments,
      ..
    } = group
    else {
      return None;
    };

    let variable_index = owner.checked_sub(4)?;
    let shape = [Kind::Variable, Kind::Equal, Kind::New, Kind::Name];
    let matches_shape =
      code.tokens[variable_index..*owner].iter().map(|token| token.kind).eq(shape);

    let is_statement = starts_statement(previous_kind(code.tokens, variable_index));
    let assigns_new_object = matches_shape && is_statement;
    if !assigns_new_object {
      return None;
    }

    let variable = code.tokens[variable_index].span;
    return Some(Construction {
      anchor: variable.start,
      variable,
      class: *class,
      arguments: *arguments,
    });
  });

  return found.collect();
}

fn member(code: &Code<'_>, index: usize) -> Result<Option<Member>, &'static str> {
  let token = code.tokens[index];
  let opener = enclosing_opener(code.tokens, index);
  let is_in_body = opener.is_some_and(|opener| code.tokens[opener].kind == Kind::OpenBrace);
  if !is_in_body {
    return Ok(None);
  }

  let anchor = code.tokens[declaration_start(code.tokens, index)].span.start;
  if token.kind == Kind::Const {
    let Some(candidate) = code.tokens.get(index + 1) else {
      return Ok(None);
    };

    let span = type_span(code, candidate.span.start, follows_constant_type)?;
    return Ok(span.map(|type_span| Member { anchor, type_span }));
  }

  let previous = previous_kind(code.tokens, index);
  let starts_member = matches!(
    previous,
    None | Some(Kind::OpenBrace | Kind::CloseBrace | Kind::Semicolon | Kind::CloseBracket)
  );

  let is_first_modifier = token.kind == Kind::Modifier && starts_member;
  if !is_first_modifier {
    return Ok(None);
  }

  let rest = &code.tokens[index..];
  let modifier_count = rest.iter().take_while(|following| following.kind == Kind::Modifier).count();
  let Some(candidate) = rest.get(modifier_count) else {
    return Ok(None);
  };

  let cannot_be_type = matches!(
    candidate.kind,
    Kind::Function
      | Kind::Fn
      | Kind::Const
      | Kind::Class
      | Kind::Interface
      | Kind::Trait
      | Kind::Variable
      | Kind::DoubleColon
      | Kind::Semicolon
      | Kind::OpenBrace
  );

  if cannot_be_type {
    return Ok(None);
  }

  let span = type_span(code, candidate.span.start, follows_property_type)?;
  return Ok(span.map(|type_span| Member { anchor, type_span }));
}

fn token_text<'a>(code: &Code<'a>, token: &Token) -> &'a [u8] {
  return &code.source[token.span.start..token.span.end];
}

fn follows_alias_type(rest: &[u8]) -> bool {
  return rest.starts_with(b";") || rest.starts_with(b"?>");
}

fn alias_declaration(
  code: &Code<'_>,
  index: usize,
) -> Result<Option<AliasDeclaration>, StripError> {
  let keyword = code.tokens[index];
  let is_type_keyword = token_text(code, &keyword) == b"type";
  let is_statement = starts_statement(previous_kind(code.tokens, index));
  let after_name = kind_at(code.tokens, index + 2);
  let names_alias = kind_at(code.tokens, index + 1) == Some(Kind::Name)
    && matches!(after_name, Some(Kind::Equal | Kind::LessThan));

  let is_declaration = is_type_keyword && is_statement && names_alias;
  if !is_declaration {
    return Ok(None);
  }

  let fail = |reason| StripError::at(code.source, keyword.span.start, reason);
  let name = token_text(code, &code.tokens[index + 1]);
  let is_qualified = name.contains(&b'\\');
  if is_qualified {
    return Err(fail("a type alias name cannot contain a namespace"));
  }

  let declares_params = after_name == Some(Kind::LessThan);
  let params = if declares_params {
    type_params(code.source, code.tokens[index + 2].span.start).map_err(fail)?
  } else {
    TypeParamList { span: code.tokens[index + 1].span, params: Vec::new() }
  };

  let is_constrained = params.params.iter().any(|param| {
    return param.bound.is_some() || param.variance != Variance::Invariant;
  });

  if is_constrained {
    return Err(fail("a type alias parameter cannot have a bound or a variance"));
  }

  let equal_index = first_token_from(code.tokens, params.span.end);
  let has_equal = kind_at(code.tokens, equal_index) == Some(Kind::Equal);
  if !has_equal {
    return Err(fail("expected `=` after the type alias name"));
  }

  let body = code.tokens.get(equal_index + 1).ok_or_else(|| fail("cannot read this type alias"))?;
  let body_span = type_span(code, body.span.start, follows_alias_type).map_err(fail)?;
  let body_span = body_span.ok_or_else(|| fail("cannot read this type alias"))?;
  let after_index = first_token_from(code.tokens, body_span.end);
  let ends_with_semicolon = kind_at(code.tokens, after_index) == Some(Kind::Semicolon);
  let end = if ends_with_semicolon { code.tokens[after_index].span.end } else { body_span.end };
  let removal = ByteSpan { start: keyword.span.start, end };
  let params = params.params;
  return Ok(Some(AliasDeclaration { removal, name: name.to_vec(), params, body: body_span }));
}

pub fn find_sites(source: &[u8], tokens: &[Token]) -> Result<Sites, StripError> {
  let code = Code { source, tokens };
  let indices = 0..tokens.len();
  let names = find_names(&code)?;
  let groups = indices
    .clone()
    .map(|index| groups_at(&code, index))
    .flatten_ok()
    .collect::<Result<Vec<_>, _>>()?;

  let angle_spans = groups.iter().map(AngleGroup::removal).collect::<Vec<_>>();
  let function_keywords = indices.clone().filter(|index| {
    let is_function = matches!(tokens[*index].kind, Kind::Function | Kind::Fn);
    return is_function && is_declaration_keyword(tokens, *index);
  });

  let functions = function_keywords
    .map(|index| function_header(&code, &groups, &angle_spans, index))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

  let class_scopes =
    groups.iter().map(|group| class_scope(&code, group)).collect::<Result<Vec<_>, _>>()?;

  let classes = class_headers(&code, &groups)?;
  let alias_declarations = indices
    .clone()
    .filter(|index| tokens[*index].kind == Kind::Name)
    .map(|index| alias_declaration(&code, index))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

  let members = indices
    .filter(|index| matches!(tokens[*index].kind, Kind::Const | Kind::Modifier))
    .map(|index| {
      member(&code, index)
        .map_err(|reason| StripError::at(source, tokens[index].span.start, reason))
    })
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

  let function_scopes = functions.iter().filter_map(|function| function.scope.clone());
  let scopes = function_scopes.chain(class_scopes.into_iter().flatten()).collect();
  let type_imports =
    names.statements.iter().filter(|statement| statement.kind == ImportKind::TypeAlias);

  let imported_aliases = type_imports.clone().flat_map(|statement| statement.imports.iter());
  let declared_aliases = alias_declarations.iter().map(|declaration| declaration.name.clone());
  let aliases =
    declared_aliases.chain(imported_aliases.map(|import| import.local.clone())).collect();

  let alias_removals = alias_declarations.iter().map(|declaration| declaration.removal);
  let import_removals = type_imports.map(|statement| statement.span);
  let removals = angle_spans.into_iter().chain(alias_removals).chain(import_removals).collect();
  return Ok(Sites {
    removals,
    declarations: Declarations { scopes, aliases },
    functions,
    classes,
    trait_uses: trait_uses(&code, &groups),
    members,
    constructions: constructions(&code, &groups),
    alias_declarations,
    names,
  });
}

impl Sites {
  pub fn type_spans(&self) -> Vec<ByteSpan> {
    let parameters = self.functions.iter().flat_map(|function| function.parameters.iter());
    let parameter_types = parameters.map(|parameter| parameter.type_span);
    let return_types = self.functions.iter().filter_map(|function| function.return_type);
    let member_types = self.members.iter().map(|member| member.type_span);
    return parameter_types.chain(return_types).chain(member_types).sorted().dedup().collect();
  }
}
