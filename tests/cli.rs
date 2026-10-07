#![allow(clippy::needless_return)]

use std::process::Command;
use std::process::Output;

fn typedhp() -> Command {
  return Command::new(env!("CARGO_BIN_EXE_typedhp"));
}

fn text(bytes: &[u8]) -> String {
  return String::from_utf8_lossy(bytes).into_owned();
}

fn run_with_stdin(arguments: &[&str], input: &[u8]) -> std::io::Result<Output> {
  let folder = tempfile::tempdir()?;
  let input_path = folder.path().join("stdin.php");
  std::fs::write(&input_path, input)?;
  let stdin = std::fs::File::open(&input_path)?;
  return typedhp().args(arguments).stdin(stdin).output();
}

#[test]
fn prints_a_file_with_its_types_stripped() -> std::io::Result<()> {
  let folder = tempfile::tempdir()?;
  let path = folder.path().join("first.php");
  std::fs::write(
    &path,
    "<?php\nfunction first<T>(list<T> $items): ?T { return $items[0] ?? null; }\n",
  )?;

  let output = typedhp().arg("strip").arg(&path).output()?;
  assert_eq!(output.status.code(), Some(0));
  assert_eq!(
    text(&output.stdout),
    "<?php\nfunction first(array $items): mixed { return $items[0] ?? null; }\n"
  );

  assert_eq!(text(&output.stderr), "");
  return Ok(());
}

#[test]
fn strips_php_read_from_stdin() -> std::io::Result<()> {
  let output = run_with_stdin(
    &["strip", "--stdin", "app/Ids.php"],
    b"<?php\ntype Id = PositiveInt;\nfunction id(Id $id): Id { return $id; }\n",
  )?;

  assert_eq!(output.status.code(), Some(0));
  assert_eq!(text(&output.stdout), "<?php\n\nfunction id(mixed $id): mixed { return $id; }\n");
  return Ok(());
}

#[test]
fn reports_a_broken_type_with_its_file_and_line() -> std::io::Result<()> {
  let output = run_with_stdin(
    &["strip", "--stdin", "app/Broken.php"],
    b"<?php\n\nfunction broken<T(T $x) {}\n",
  )?;

  assert_eq!(output.status.code(), Some(1));
  assert_eq!(text(&output.stdout), "");
  assert_eq!(
    text(&output.stderr),
    "app/Broken.php:3: expected `,` or `>` in a type parameter list\n"
  );

  return Ok(());
}

#[test]
fn reports_an_unreadable_file_apart_from_type_errors() -> std::io::Result<()> {
  let folder = tempfile::tempdir()?;
  let path = folder.path().join("missing.php");
  let output = typedhp().arg("strip").arg(&path).output()?;
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    text(&output.stderr),
    format!("{}: No such file or directory (os error 2)\n", path.display())
  );

  return Ok(());
}

#[test]
fn prints_usage_for_unknown_arguments() -> std::io::Result<()> {
  let output = typedhp().arg("strip-types").output()?;
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    text(&output.stderr),
    "usage:
  typedhp strip <file>          print <file> with its types stripped
  typedhp strip --stdin <name>  strip PHP read from stdin; <name> labels errors
  typedhp install <folder>      install typedhp, the php and mago shims and the loader into <folder>
  typedhp check [<mago args>]   type-check the project in this folder with Mago, through .typedhp/check/
  typedhp mago <mago args>      run Mago; format, lint, analyze and guard work on typed files through .typedhp/
"
  );

  return Ok(());
}
