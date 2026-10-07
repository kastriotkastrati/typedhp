#![allow(clippy::needless_return)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
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
  return Ok(folder);
}

fn mago(project: &Path, arguments: &[&str]) -> std::io::Result<Output> {
  return Command::new(env!("CARGO_BIN_EXE_typedhp"))
    .arg("mago")
    .args(arguments)
    .current_dir(project)
    .env_remove("TYPEDHP_MAGO")
    .output();
}

fn messy_source() -> &'static str {
  return r#"<?php
namespace App;
use Zeta\Thing;
use type App\Shapes\{Point,   Line as Segment};
use Alpha\Other;
type   UserId =   PositiveInt;
type Result<T, E = string> = Ok<T>|Err<E>;
final   class Stack<T> implements Holder<T> {
    public const list<int> SIZES = [1,2];
  private ?list<T>   $items = null;
  public function map<U>(\Closure(T): U $mapper)  : Stack<U> { return new Stack::<U>(); }
    public function first(): ?T { $pick = fn<V>(list<V> $values): ?V => $values[0] ?? null; return $pick([1]); }
}
"#;
}

fn formatted_source() -> &'static str {
  return r#"<?php

namespace App;

use Alpha\Other;
use type App\Shapes\{Point, Line as Segment};
use Zeta\Thing;

type UserId = PositiveInt;
type Result<T, E = string> = Ok<T>|Err<E>;
final class Stack<T> implements Holder<T>
{
    public const list<int> SIZES = [1, 2];

    private ?list<T> $items = null;

    public function map<U>(\Closure(T): U $mapper): Stack<U>
    {
        return new Stack::<U>();
    }

    public function first(): ?T
    {
        $pick = fn<V>(list<V> $values): ?V => $values[0] ?? null;
        return $pick([1]);
    }
}
"#;
}

#[test]
fn formats_typed_files_and_keeps_their_types() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/Stack.php", messy_source())?;
  let first = mago(folder.path(), &["format"])?;
  let formatted = std::fs::read_to_string(folder.path().join("src/Stack.php"))?;
  assert_eq!(first.status.code(), Some(0), "{}", text(&first.stderr));
  assert_eq!(formatted, formatted_source());

  let second = mago(folder.path(), &["format", "src/Stack.php"])?;
  let reformatted = std::fs::read_to_string(folder.path().join("src/Stack.php"))?;
  assert_eq!(second.status.code(), Some(0), "{}", text(&second.stderr));
  assert_eq!(reformatted, formatted_source());
  return Ok(());
}

#[test]
fn reports_unformatted_typed_files_without_changing_them() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/Stack.php", messy_source())?;
  let output = mago(folder.path(), &["format", "--check"])?;
  let source = std::fs::read_to_string(folder.path().join("src/Stack.php"))?;
  assert_eq!(output.status.code(), Some(1));
  assert_eq!(source, messy_source());

  write(folder.path(), "src/Stack.php", formatted_source())?;
  let formatted = mago(folder.path(), &["format", "--check"])?;
  assert_eq!(formatted.status.code(), Some(0));
  return Ok(());
}

#[test]
fn maps_lint_issues_to_the_lines_of_the_typed_file() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/same.php",
    r#"<?php

declare(strict_types=1);

namespace App;

function same<T>(list<T> $left, list<T> $right): bool {
    return $left == $right;
}
"#,
  )?;

  let output = mago(folder.path(), &["lint"])?;
  assert_eq!(
    text(&output.stdout),
    "src/same.php:8: warning[identity-comparison]: Use identity comparison `===` instead of equality comparison `==`.\n"
  );

  let errors = text(&output.stderr);
  assert!(errors.starts_with("typedhp: running mago lint in .typedhp/lint\n"), "{errors}");
  assert!(errors.ends_with("typedhp: found 1 issues\n"), "{errors}");
  return Ok(());
}

#[test]
fn runs_mago_unchanged_in_a_project_without_typed_files() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/plain.php",
    "<?php\n\nfunction same($a, $b) {\n    return $a == $b;\n}\n",
  )?;

  let through_typedhp = mago(folder.path(), &["lint", "--reporting-format", "emacs"])?;
  let direct = Command::new("mago")
    .args(["lint", "--reporting-format", "emacs"])
    .current_dir(folder.path())
    .output()?;

  assert_eq!(text(&through_typedhp.stdout), text(&direct.stdout));
  assert_eq!(through_typedhp.status.code(), direct.status.code());
  assert!(!folder.path().join(".typedhp").exists());
  return Ok(());
}

fn fixable_source() -> &'static str {
  return r#"<?php

declare(strict_types=1);

namespace App;

final class Ids<T>
{
    /** @param list<T> $items the items */
    public function __construct(private list<T> $items) {}

    /** Lists the ids. */
    public function ids(list<int> $ids = null): list<int>
    {
        $fallback = array(1, 2);
        return $ids ?? $fallback;
    }
}
"#;
}

fn fixed_source() -> &'static str {
  return r#"<?php

declare(strict_types=1);

namespace App;

final class Ids<T>
{
    /** @param list<T> $items the items */
    public function __construct(private list<T> $items) {}

    /** Lists the ids. */
    public function ids(?list<int> $ids = null): list<int>
    {
        $fallback = [1, 2];
        return $ids ?? $fallback;
    }
}
"#;
}

#[test]
fn fixes_typed_files_and_keeps_their_types() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/Ids.php", fixable_source())?;
  let output = mago(folder.path(), &["lint", "--fix"])?;
  let fixed = std::fs::read_to_string(folder.path().join("src/Ids.php"))?;
  assert_eq!(text(&output.stdout), "");
  assert_eq!(
    text(&output.stderr),
    "typedhp: running mago lint in .typedhp/lint\ntypedhp: fixed 1 files\n"
  );

  assert_eq!(output.status.code(), Some(0));
  assert_eq!(fixed, fixed_source());

  let again = mago(folder.path(), &["lint"])?;
  assert!(text(&again.stderr).ends_with("typedhp: no issues found\n"), "{}", text(&again.stderr));
  return Ok(());
}

#[test]
fn keeps_the_plain_copy_out_of_the_typed_file() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/Ids.php", fixable_source())?;
  let output = mago(folder.path(), &["lint", "--fix"])?;
  let copy = std::fs::read_to_string(folder.path().join(".typedhp/lint/src/Ids.php"))?;
  let fixed = std::fs::read_to_string(folder.path().join("src/Ids.php"))?;
  assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
  assert_eq!(
    copy,
    r#"<?php

declare(strict_types=1);

namespace App;

/** @template T */ final class Ids
{
    /** @param list<T> $items the items
     * @param list<T> $items
     */
    public function __construct(private array $items) {}

    /** Lists the ids.
     * @param list<int> $ids
     * @return list<int>
     */
    public function ids(array $ids = null): array
    {
        $fallback = array(1, 2);
        return $ids ?? $fallback;
    }
}
"#
  );

  assert_eq!(fixed, fixed_source());
  return Ok(());
}

#[test]
fn skips_a_fix_that_changes_code_typedhp_rewrote() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/pick.php",
    "<?php\n\ndeclare(strict_types=1);\n\nfunction pick(Int|list<string> $value): Int\n{\n    return 1;\n}\n",
  )?;

  let output = mago(folder.path(), &["lint", "--fix"])?;
  let fixed = std::fs::read_to_string(folder.path().join("src/pick.php"))?;
  assert_eq!(
    text(&output.stdout),
    "src/pick.php:5: error[typedhp]: skipped the `lowercase-type-hint` fix, because it changes code that typedhp rewrote\n"
  );

  assert_eq!(output.status.code(), Some(1));
  assert_eq!(
    fixed,
    "<?php\n\ndeclare(strict_types=1);\n\nfunction pick(Int|list<string> $value): int\n{\n    return 1;\n}\n"
  );

  return Ok(());
}

#[test]
fn applies_riskier_analyzer_fixes_only_when_asked() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "src/Name.php",
    r#"<?php

declare(strict_types=1);

namespace App;

final class Name<T>
{
    public function __construct(private list<T> $items) {}

    public function name(): string
    {
        return (string) 'name';
    }
}
"#,
  )?;

  let safe = mago(folder.path(), &["analyze", "--fix", "--fail-on-remaining"])?;
  let fixed = std::fs::read_to_string(folder.path().join("src/Name.php"))?;
  assert_eq!(
    text(&safe.stderr),
    "typedhp: running mago analyze in .typedhp/check
typedhp: skipped 1 potentially unsafe fixes; use `--potentially-unsafe` or `--unsafe` to apply them
typedhp: fixed 1 files
typedhp: 1 issues need fixing by hand
"
  );

  assert_eq!(safe.status.code(), Some(1));
  assert_eq!(
    fixed,
    r#"<?php

declare(strict_types=1);

namespace App;

final class Name<T>
{
    public function __construct(private list<T> $items) {}

    public function name(): string
    {
        return  'name';
    }
}
"#
  );

  let risky = mago(folder.path(), &["analyze", "--fix", "--potentially-unsafe"])?;
  let renamed = std::fs::read_to_string(folder.path().join("src/Name.php"))?;
  assert_eq!(risky.status.code(), Some(0), "{}", text(&risky.stderr));
  assert_eq!(
    renamed,
    r#"<?php

declare(strict_types=1);

namespace App;

final class Name<T>
{
    public function __construct(private list<T> $_items) {}

    public function name(): string
    {
        return  'name';
    }
}
"#
  );

  return Ok(());
}

#[test]
fn refuses_to_preview_fixes_to_typed_files() -> std::io::Result<()> {
  let folder = project()?;
  write(folder.path(), "src/ids.php", "<?php\n\nfunction ids(list<int> $ids): void {}\n")?;
  let output = mago(folder.path(), &["lint", "--fix", "--dry-run"])?;
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    text(&output.stderr),
    "typedhp: `mago lint --dry-run` does not work in a project with typed files yet\n"
  );

  return Ok(());
}

#[test]
fn links_vendor_folders_below_the_project_root() -> std::io::Result<()> {
  let folder = project()?;
  write(
    folder.path(),
    "mago.toml",
    "php-version = \"8.5.0\"\n\n[source]\npaths = [\"web/src\"]\nincludes = [\"web/vendor\"]\n",
  )?;

  write(folder.path(), "web/.gitignore", "/vendor\n")?;
  write(
    folder.path(),
    "web/vendor/acme/Clock.php",
    "<?php\n\nnamespace Acme;\n\nfinal class Clock\n{\n    public function now(): int {\n        return 1;\n    }\n}\n",
  )?;

  write(
    folder.path(),
    "web/src/time.php",
    "<?php\n\nnamespace App;\n\nfunction now(\\Acme\\Clock $clock): PositiveInt {\n    return $clock->now();\n}\n",
  )?;

  let output = mago(folder.path(), &["analyze"])?;
  assert_eq!(
    text(&output.stdout),
    "web/src/time.php:6: error[invalid-return-statement]: Invalid return type for function `App\\now`: expected `PositiveInt`, but found `int`.\n"
  );

  let linked = std::fs::read_link(folder.path().join(".typedhp/check/web/vendor"))?;
  assert_eq!(linked, folder.path().join("web/vendor"));
  return Ok(());
}

#[test]
fn runs_the_mago_named_in_typedhp_mago_unchanged() -> std::io::Result<()> {
  let folder = project()?;
  let fake = folder.path().join("fake-mago");
  std::fs::write(&fake, "#!/bin/sh\necho \"fake mago: $*\"\n")?;
  std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))?;
  let output = Command::new(env!("CARGO_BIN_EXE_typedhp"))
    .args(["mago", "--config", "mago.toml", "list-files"])
    .current_dir(folder.path())
    .env("TYPEDHP_MAGO", &fake)
    .output()?;

  assert_eq!(text(&output.stdout), "fake mago: --config mago.toml list-files\n");
  assert_eq!(output.status.code(), Some(0));
  return Ok(());
}

#[test]
fn skips_the_installed_mago_shim_when_it_looks_for_mago() -> std::io::Result<()> {
  let folder = project()?;
  let home = folder.path().join("typedhp-home");
  let installed = Command::new(env!("CARGO_BIN_EXE_typedhp")).arg("install").arg(&home).output()?;
  assert_eq!(installed.status.code(), Some(0), "{}", text(&installed.stderr));

  write(
    folder.path(),
    "src/ids.php",
    "<?php\n\ndeclare(strict_types=1);\n\nfunction ids(list<int> $ids): bool {\n    return $ids == [];\n}\n",
  )?;

  let inherited = std::env::var_os("PATH").ok_or(std::io::Error::other("PATH is not set"))?;
  let shim_folder: PathBuf = home.join("bin");
  let path = std::env::join_paths(
    std::iter::once(shim_folder.clone()).chain(std::env::split_paths(&inherited)),
  )
  .map_err(std::io::Error::other)?;

  let output = Command::new(shim_folder.join("mago"))
    .arg("lint")
    .current_dir(folder.path())
    .env("PATH", path)
    .env_remove("TYPEDHP_MAGO")
    .output()?;

  assert_eq!(
    text(&output.stdout),
    "src/ids.php:6: warning[identity-comparison]: Use identity comparison `===` instead of equality comparison `==`.\n"
  );

  return Ok(());
}
