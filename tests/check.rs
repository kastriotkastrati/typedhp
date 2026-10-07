#![allow(clippy::needless_return)]

use std::path::Path;
use std::process::Command;
use std::process::Output;
use tempfile::TempDir;

fn text(bytes: &[u8]) -> String {
  return String::from_utf8_lossy(bytes).into_owned();
}

fn write(project: &Path, relative: &str, contents: &str) -> std::io::Result<()> {
  let path = project.join(relative);
  let parent = path.parent().ok_or(std::io::Error::other("fixture path has no parent"))?;
  std::fs::create_dir_all(parent)?;
  return std::fs::write(path, contents);
}

fn project() -> std::io::Result<TempDir> {
  let folder = tempfile::tempdir()?;
  write(folder.path(), "mago.toml", "php-version = \"8.5.0\"\n\n[source]\npaths = [\"src\"]\n")?;
  write(
    folder.path(),
    "src/Box.php",
    r#"<?php

namespace App;

final class Box<T>
{
    public function __construct(private T $value) {}

    public function get(): T {
        return $this->value;
    }
}
"#,
  )?;

  return Ok(folder);
}

fn check(project: &Path, arguments: &[&str]) -> std::io::Result<Output> {
  return Command::new(env!("CARGO_BIN_EXE_typedhp"))
    .arg("check")
    .args(arguments)
    .current_dir(project)
    .output();
}

fn assert_ran_mago(output: &Output, summary: &str) {
  let errors = text(&output.stderr);
  assert!(errors.starts_with("typedhp: running mago analyze in .typedhp/check\n"), "{errors}");
  assert!(errors.ends_with(summary), "{errors}");
}

#[test]
fn reports_mago_errors_at_their_original_lines() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/use.php",
    r#"<?php

namespace App;

/**
 * Doubles each number.
 */
function double(List<int> $numbers): List<int> {
    return array_map(fn(int $number): int => $number * 2, $numbers);
}

$box = new Box::<string>('text');
echo intdiv($box->get(), 2);
echo double(['a'])[0];
"#,
  )?;

  let output = check(folder.path(), &[])?;
  assert_eq!(
    text(&output.stdout),
    "src/use.php:13: error[invalid-argument]: Invalid argument type for argument #1 of `intdiv`: expected `int`, but found `string`.
src/use.php:14: error[possibly-invalid-argument]: Possible argument type mismatch for argument #1 of `App\\double`: expected `List<int>`, but possibly received `List{string('a')}`.
"
  );

  assert_ran_mago(&output, "typedhp: found 2 issues\n");
  assert_eq!(output.status.code(), Some(1));
  return Ok(());
}

#[test]
fn reports_a_broken_type_once_and_checks_the_other_files() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/broken.php", "<?php\n\nfunction broken<T(T $x) {}\n")?;
  write(
    folder.path(),
    "src/use.php",
    "<?php\n\nnamespace App;\n\necho intdiv((new Box::<string>('a'))->get(), 2);\n",
  )?;

  let output = check(folder.path(), &[])?;
  assert_eq!(
    text(&output.stdout),
    "src/broken.php:3: error[typedhp]: expected `,` or `>` in a type parameter list
src/use.php:5: error[invalid-argument]: Invalid argument type for argument #1 of `intdiv`: expected `int`, but found `string`.
"
  );

  assert_eq!(output.status.code(), Some(1));
  return Ok(());
}

#[test]
fn reports_a_type_alias_declared_twice() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/a.php", "<?php\nnamespace App;\ntype Id = int;\n")?;
  write(folder.path(), "src/b.php", "<?php\nnamespace App;\n\ntype Id = string;\n")?;
  let output = check(folder.path(), &[])?;
  assert_eq!(
    text(&output.stdout),
    "src/b.php:4: error[typedhp]: type alias `\\App\\Id` is already declared at src/a.php:3\n"
  );

  assert_eq!(output.status.code(), Some(1));
  return Ok(());
}

#[test]
fn passes_a_clean_project_and_keeps_the_mirror_out_of_git() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/use.php",
    "<?php\n\nnamespace App;\n\nfunction label(NonEmptyString $name): string {\n    return $name;\n}\n",
  )?;

  let output = check(folder.path(), &[])?;
  let mirrored = std::fs::read_to_string(folder.path().join(".typedhp/check/src/use.php"))?;
  let ignored = std::fs::read_to_string(folder.path().join(".typedhp/.gitignore"))?;
  let linked_config = std::fs::read_link(folder.path().join(".typedhp/check/mago.toml"))?;
  assert_eq!(text(&output.stdout), "");
  assert_ran_mago(&output, "typedhp: no issues found\n");
  assert_eq!(output.status.code(), Some(0));
  assert_eq!(
    mirrored,
    "<?php\n\nnamespace App;\n\n/** @param non-empty-string $name */ function label(string $name): string {\n    return $name;\n}\n"
  );

  assert_eq!(ignored, "*\n");
  assert_eq!(linked_config, folder.path().join("mago.toml"));
  return Ok(());
}

#[test]
fn hides_mago_warnings_about_docblocks_that_typedhp_wrote() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/use.php",
    "<?php\n\nnamespace App;\n\n$box = new Box::<int>(3);\necho $box->get() + 1;\n",
  )?;

  let output = check(folder.path(), &[])?;
  assert_eq!(text(&output.stdout), "");
  assert_ran_mago(&output, "typedhp: no issues found\n");
  assert_eq!(output.status.code(), Some(0));
  return Ok(());
}

#[test]
fn passes_extra_arguments_to_mago() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/one.php", "<?php\n\nnamespace App;\n\necho intdiv('one', 2);\n")?;
  write(folder.path(), "src/two.php", "<?php\n\nnamespace App;\n\necho intdiv('two', 2);\n")?;
  let output = check(folder.path(), &["src/two.php"])?;
  assert_eq!(
    text(&output.stdout),
    "src/two.php:5: error[invalid-argument]: Invalid argument type for argument #1 of `intdiv`: expected `int`, but found `string('two')`.\n"
  );

  return Ok(());
}

#[test]
fn rebuilds_the_mirror_without_deleted_files() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/old.php", "<?php\n\nnamespace App;\n\necho intdiv('old', 2);\n")?;
  let first = check(folder.path(), &[])?;
  std::fs::remove_file(folder.path().join("src/old.php"))?;
  let second = check(folder.path(), &[])?;
  assert_eq!(first.status.code(), Some(1));
  assert_eq!(text(&second.stdout), "");
  assert_eq!(second.status.code(), Some(0));
  assert!(!folder.path().join(".typedhp/check/src/old.php").exists());
  return Ok(());
}

#[test]
fn narrows_a_generic_result_type_alias() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/result.php",
    r#"<?php

namespace App;

type Result<T, E = string> = Ok<T>|Err<E>;

final readonly class Ok<+T>
{
    public true $ok;
    public null $error;

    public function __construct(public T $data) {
        $this->ok = true;
        $this->error = null;
    }
}

final readonly class Err<+E>
{
    public false $ok;
    public null $data;

    public function __construct(public E $error) {
        $this->ok = false;
        $this->data = null;
    }
}
"#,
  )?;

  write(
    folder.path(),
    "src/use.php",
    r#"<?php

namespace App;

use type App\Result;

function parse(string $text): Result<PositiveInt> {
    $number = (int) $text;
    return $number > 0 ? new Ok($number) : new Err('not a number');
}

function doubled(string $text): int {
    $parsed = parse($text);
    if (!$parsed->ok) {
        return 0;
    }

    return $parsed->data * 2;
}

function careless(string $text): PositiveInt {
    return parse($text)->data;
}
"#,
  )?;

  let output = check(folder.path(), &[])?;
  assert_eq!(
    text(&output.stdout),
    "src/use.php:22: error[nullable-return-statement]: Function `App\\careless` is declared to return `PositiveInt` but possibly returns a nullable value (inferred as `null|PositiveInt`).
src/use.php:22: error[invalid-return-statement]: Invalid return type for function `App\\careless`: expected `PositiveInt`, but found `null|PositiveInt`.
"
  );

  assert_eq!(output.status.code(), Some(1));
  return Ok(());
}
