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

function total(list<int> $numbers, int $limit): int {
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

/** @param list<int> $numbers */ function total(array $numbers, int $limit): int {
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
      "/** @param list<int> $numbers */ ".to_string(),
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

    public const list<int> SIZES = [1];
    private ?T $value = null;

    public function __construct(private non-empty-string $name) {}
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

    /** @var list<int> */ public const array SIZES = [1];
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
fn adds_tags_to_a_one_line_docblock() -> Result<(), StripError> {
  let source = "<?php\n    /** @var Command $this */\n    $numbers = new Stack::<int>();\n";
  let desugared = desugar_project(&[source])?;
  assert_eq!(
    text(&desugared.code),
    "<?php\n    /** @var Command $this \n     * @var Stack<int> $numbers\n     */\n    $numbers = new Stack();\n"
  );

  assert_eq!(desugared.lines, vec![1, 2, 2, 2, 3, 4]);
  return Ok(());
}

#[test]
fn expands_type_aliases_from_other_files_with_qualified_names() -> Result<(), StripError> {
  let types = r#"<?php

namespace App\Types;

use App\Models\User;

type Id = positive-int;
type Users = list<User>;
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
 * @param (array<(positive-int), (list<\App\Models\User>)>) $index
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
  let source = "<?php\ntype Tree = list<Tree>;\nfunction walk(Tree $tree): void {}\n";
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
