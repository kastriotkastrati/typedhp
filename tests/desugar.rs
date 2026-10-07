#![allow(clippy::needless_return)]

use typedhp::Desugared;
use typedhp::StripError;

fn desugar_project(files: &[&str]) -> Result<Desugared, StripError> {
  let aliases = files
    .iter()
    .map(|source| typedhp::type_aliases(source.as_bytes()))
    .collect::<Result<Vec<_>, _>>()?;

  let table = typedhp::alias_table(aliases.iter().flatten());
  let last = files.last().ok_or(StripError { line: 0, reason: "no files" })?;
  return typedhp::desugar(last.as_bytes(), &table);
}

fn text(bytes: &[u8]) -> String {
  return String::from_utf8_lossy(bytes).into_owned();
}

#[test]
fn writes_function_types_as_docblock_tags_and_maps_lines_back() -> Result<(), StripError> {
  let source = r#"<?php

/**
 * Picks the first value.
 */
function first<T: object>(T ...$values): ?T {
    return $values[0] ?? null;
}

function total(List<int> $numbers, int $limit): int {
    return count($numbers);
}
"#;

  let desugared = desugar_project(&[source])?;
  assert_eq!(
    text(&desugared.code),
    r#"<?php

/**
 * Picks the first value.
 * @template T of object
 * @param T ...$values
 * @return ?T
 */
function first(object ...$values): ?object {
    return $values[0] ?? null;
}

/** @param List<int> $numbers */ function total(array $numbers, int $limit): int {
    return count($numbers);
}
"#
  );

  assert_eq!(desugared.lines, vec![1, 2, 3, 4, 5, 5, 5, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
  let docblocks =
    desugared.docblocks.iter().map(|span| text(&desugared.code[span.start..span.end]));

  assert_eq!(
    docblocks.collect::<Vec<_>>(),
    vec![
      " * @template T of object\n * @param T ...$values\n * @return ?T\n".to_string(),
      "/** @param List<int> $numbers */ ".to_string(),
    ]
  );

  return Ok(());
}

#[test]
fn writes_class_types_as_docblock_tags() -> Result<(), StripError> {
  let source = r#"<?php

namespace App;

#[Attribute]
final class Box<+T> extends Base<T> implements Holder<T>, Countable
{
    use Labels<T>;

    public const List<int> SIZES = [1];
    private ?T $value = null;

    public function __construct(private NonEmptyString $name) {}
}

function make(): void {
    $box = new Box::<int>('box');
    echo $box->count();
}
"#;

  let desugared = desugar_project(&[source])?;
  assert_eq!(
    text(&desugared.code),
    r#"<?php

namespace App;

/**
 * @template-covariant T
 * @extends Base<T>
 * @implements Holder<T>
 */
#[Attribute]
final class Box extends Base implements Holder, Countable
{
    /** @use Labels<T> */ use Labels;

    /** @var List<int> */ public const array SIZES = [1];
    /** @var ?T */ private mixed $value = null;

    /** @param non-empty-string $name */ public function __construct(private string $name) {}
}

function make(): void {
    /** @var Box<int> $box */ $box = new Box('box');
    echo $box->count();
}
"#
  );

  assert_eq!(desugared.lines[4..11], [5, 5, 5, 5, 5, 5, 6]);
  return Ok(());
}

#[test]
fn adds_tags_to_a_one_line_docblock_without_trailing_blanks() -> Result<(), StripError> {
  let source = "<?php\n    /** @var Command $this */\n    $numbers = new Stack::<int>();\n";
  let desugared = desugar_project(&[source])?;
  assert_eq!(
    text(&desugared.code),
    "<?php\n    /** @var Command $this\n     * @var Stack<int> $numbers\n     */\n    $numbers = new Stack();\n"
  );

  assert_eq!(desugared.lines, vec![1, 2, 2, 2, 3, 4]);
  return Ok(());
}

#[test]
fn expands_type_aliases_from_other_files_with_qualified_names() -> Result<(), StripError> {
  let types = r#"<?php

namespace App\Types;

use App\Models\User;

type Id = PositiveInt;
type Users = List<User>;
type Index = array<Id, Users>;
"#;

  let source = r#"<?php

namespace App;

use type App\Types\{Index, Id as Key};

function find(Index $index, Key $key): ?Key {
    return null;
}
"#;

  let desugared = desugar_project(&[types, source])?;
  assert_eq!(
    text(&desugared.code),
    r#"<?php

namespace App;



/**
 * @param (array<(positive-int), (List<\App\Models\User>)>) $index
 * @param (positive-int) $key
 * @return ?(positive-int)
 */
function find(mixed $index, mixed $key): mixed {
    return null;
}
"#
  );

  return Ok(());
}

#[test]
fn writes_title_case_types_the_way_mago_spells_them() -> Result<(), StripError> {
  let aliases = "<?php\nnamespace App\\Types;\n\ntype Ids = NonEmptyList<PositiveInt>;\n";
  let source = r#"<?php
namespace App;

use App\Values\ArrayKey;
use type App\Types\Ids;

function find<T: object>(ClassString<T> $class, Ids $ids, ArrayKey $key): ?T {
    return null;
}
"#;

  let desugared = desugar_project(&[aliases, source])?;
  assert_eq!(
    text(&desugared.code),
    r#"<?php
namespace App;

use App\Values\ArrayKey;


/**
 * @template T of object
 * @param class-string<T> $class
 * @param (non-empty-list<positive-int>) $ids
 * @return ?T
 */
function find(string $class, mixed $ids, ArrayKey $key): ?object {
    return null;
}
"#
  );

  return Ok(());
}

#[test]
fn keeps_a_type_parameter_that_shares_an_alias_name() -> Result<(), StripError> {
  let source = "<?php\ntype T = int;\nfunction keep<T>(T $value): T { return $value; }\n";
  let desugared = desugar_project(&[source])?;
  assert_eq!(
    text(&desugared.code),
    "<?php\n\n/**\n * @template T\n * @param T $value\n * @return T\n */\nfunction keep(mixed $value): mixed { return $value; }\n"
  );

  return Ok(());
}

#[test]
fn reports_a_type_alias_that_refers_to_itself() {
  let source = "<?php\ntype Tree = List<Tree>;\nfunction walk(Tree $tree): void {}\n";
  let desugared = desugar_project(&[source]);
  assert_eq!(desugared, Err(StripError { line: 3, reason: "a type alias refers to itself" }));
}

#[test]
fn reports_an_imported_type_alias_that_no_file_declares() {
  let source = "<?php\nuse type App\\Missing;\n\nfunction find(Missing $value): void {}\n";
  let desugared = desugar_project(&[source]);
  assert_eq!(
    desugared,
    Err(StripError { line: 4, reason: "this type alias is not declared in the project" })
  );
}

#[test]
fn expands_generic_type_aliases_with_their_arguments_and_defaults() -> Result<(), StripError> {
  let types = r#"<?php

namespace App;

type Result<T, E = string> = Ok<T>|Err<E>;
type Pair<T> = array{T, T};
type Id = PositiveInt;
"#;

  let source = r#"<?php

namespace App;

use type App\{Result, Pair, Id};

function load(Pair<Id> $ids): Result<List<Id>> {
    return new Err('none');
}

function parse(string $text): ?Result<Pair<Id>, \Throwable> {
    return null;
}
"#;

  let desugared = desugar_project(&[types, source])?;
  assert_eq!(
    text(&desugared.code),
    r#"<?php

namespace App;



/**
 * @param (array{(positive-int), (positive-int)}) $ids
 * @return (\App\Ok<(List<(positive-int)>)>|\App\Err<string>)
 */
function load(mixed $ids): mixed {
    return new Err('none');
}

/** @return ?(\App\Ok<(array{(positive-int), (positive-int)})>|\App\Err<\Throwable>) */ function parse(string $text): mixed {
    return null;
}
"#
  );

  return Ok(());
}

#[test]
fn reports_type_arguments_that_do_not_fit_a_type_alias() {
  let types = "<?php\ntype Pair<A, B> = array{A, B};\ntype Id = int;\n";
  let cases = [
    ("Pair<int, int, int>", "too many type arguments for this type alias"),
    ("Pair<int>", "missing type arguments for this type alias"),
    ("Pair", "missing type arguments for this type alias"),
    ("Id<int>", "too many type arguments for this type alias"),
  ];

  cases.iter().for_each(|(written, reason)| {
    let source =
      format!("<?php\nuse type Pair;\nuse type Id;\nfunction take({written} $value): void {{}}\n");

    let desugared = desugar_project(&[types, &source]);
    assert_eq!(desugared, Err(StripError { line: 4, reason }), "{written}");
  });
}

#[test]
fn reports_a_type_alias_default_that_uses_a_later_parameter() {
  let source =
    "<?php\ntype Pair<A = B, B = int> = array{A, B};\nfunction take(Pair $pair): void {}\n";

  let desugared = desugar_project(&[source]);
  assert_eq!(
    desugared,
    Err(StripError { line: 3, reason: "a type alias parameter has no argument" })
  );
}

#[test]
fn maps_spans_of_the_plain_copy_back_to_the_typed_source() -> Result<(), StripError> {
  let source =
    "<?php\nfunction first<T>(List<T> $values): ?T {\n    return $values[0] ?? null;\n}\n";
  let desugared = desugar_project(&[source])?;
  let code = text(&desugared.code);
  assert_eq!(
    code,
    "<?php\n/**\n * @template T\n * @param List<T> $values\n * @return ?T\n */\nfunction first(array $values): mixed {\n    return $values[0] ?? null;\n}\n"
  );

  let at = |needle: &str| code.find(needle).ok_or(StripError { line: 0, reason: "missing" });
  let span = |start: usize, end: usize| typedhp::ByteSpan { start, end };
  let source_text = |mapped: Option<typedhp::ByteSpan>| {
    return mapped.map(|found| text(&source.as_bytes()[found.start..found.end]));
  };

  let lookup = at("$values[0]")?;
  let erased = at("array")?;
  let parameters = at("(array")?;
  let tag = at("@template")?;
  assert_eq!(
    source_text(typedhp::source_span(&desugared, span(lookup, lookup + 10))),
    Some("$values[0]".to_string())
  );

  assert_eq!(
    typedhp::source_span(&desugared, span(erased, erased)).map(|found| found.start),
    source.find("List<T>")
  );

  assert_eq!(typedhp::source_span(&desugared, span(erased, erased + 5)), None);
  assert_eq!(typedhp::source_span(&desugared, span(parameters, parameters)), None);
  assert_eq!(typedhp::source_span(&desugared, span(tag, tag + 9)), None);
  return Ok(());
}
