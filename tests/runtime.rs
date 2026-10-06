#![allow(clippy::needless_return)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use tempfile::TempDir;

struct Installation {
  _folder: TempDir,
  home: PathBuf,
  project: PathBuf,
}

fn text(bytes: &[u8]) -> String {
  return String::from_utf8_lossy(bytes).into_owned();
}

fn install() -> std::io::Result<Installation> {
  let folder = tempfile::tempdir()?;
  let home = folder.path().join("typedhp-home");
  let project = folder.path().join("project");
  let installed = Command::new(env!("CARGO_BIN_EXE_typedhp")).arg("install").arg(&home).output()?;
  assert_eq!(
    installed.status.code(),
    Some(0),
    "typedhp install failed: {}",
    text(&installed.stderr)
  );

  std::fs::create_dir_all(&project)?;
  return Ok(Installation { _folder: folder, home, project });
}

fn write(project: &Path, relative: &str, contents: &str) -> std::io::Result<()> {
  let path = project.join(relative);
  let parent = path.parent().ok_or(std::io::Error::other("fixture path has no parent"))?;
  std::fs::create_dir_all(parent)?;
  return std::fs::write(path, contents);
}

fn php(installation: &Installation, arguments: &[&str]) -> std::io::Result<Output> {
  let inherited = std::env::var_os("PATH").ok_or(std::io::Error::other("PATH is not set"))?;
  let shim_folder = installation.home.join("bin");
  let path = std::env::join_paths(
    std::iter::once(shim_folder.clone()).chain(std::env::split_paths(&inherited)),
  )
  .map_err(std::io::Error::other)?;

  return Command::new(shim_folder.join("php"))
    .args(arguments)
    .current_dir(&installation.project)
    .env("PATH", path)
    .env_remove("PHP_INI_SCAN_DIR")
    .output();
}

fn write_box_fixtures(project: &Path) -> std::io::Result<()> {
  write(
    project,
    "main.php",
    r#"<?php
require __DIR__ . '/src/Box.php';

function describe<T>(Box<T> $box): string {
    return get_class($box) . ' holds ' . $box->get();
}

echo describe(new Box::<int>(7)), "\n";
echo basename(__FILE__), ':', __LINE__, "\n";
try {
    (new Box::<string>('x'))->fail();
} catch (RuntimeException $error) {
    echo basename($error->getFile()), ':', $error->getLine(), "\n";
}
"#,
  )?;

  return write(
    project,
    "src/Box.php",
    r#"<?php

final class Box<T>
{
    public function __construct(private T $value) {}

    public function get(): T {
        return $this->value;
    }

    public function fail(): never {
        throw new RuntimeException('failed');
    }
}
"#,
  );
}

#[test]
fn runs_a_typed_script_and_its_includes_with_their_real_paths_and_lines() -> std::io::Result<()> {
  let installation = install()?;
  write_box_fixtures(&installation.project)?;
  let output = php(&installation, &["--strip-types", "main.php"])?;
  assert_eq!(text(&output.stderr), "");
  assert_eq!(text(&output.stdout), "Box holds 7\nmain.php:9\nBox.php:12\n");
  assert_eq!(output.status.code(), Some(0));
  return Ok(());
}

#[test]
fn runs_php_unchanged_without_the_flag() -> std::io::Result<()> {
  let installation = install()?;
  write_box_fixtures(&installation.project)?;
  let output = php(&installation, &["main.php"])?;
  let printed = format!("{}{}", text(&output.stdout), text(&output.stderr));
  assert_eq!(output.status.code(), Some(255));
  assert!(printed.contains("syntax error, unexpected token \"<\""), "{printed}");
  assert!(printed.contains("main.php on line 4"), "{printed}");
  return Ok(());
}

#[test]
fn reports_a_broken_type_as_a_parse_error_at_its_line() -> std::io::Result<()> {
  let installation = install()?;
  write(&installation.project, "src/broken.php", "<?php\n\nfunction broken<T(T $x) {}\n")?;
  write(
    &installation.project,
    "main.php",
    r#"<?php
try {
    require __DIR__ . '/src/broken.php';
} catch (ParseError $error) {
    echo $error->getMessage(), '|', basename($error->getFile()), ':', $error->getLine(), "\n";
}
"#,
  )?;

  let output = php(&installation, &["--strip-types", "main.php"])?;
  assert_eq!(text(&output.stderr), "");
  assert_eq!(
    text(&output.stdout),
    "typedhp: expected `,` or `>` in a type parameter list|broken.php:3\n"
  );

  return Ok(());
}

#[test]
fn leaves_files_under_vendor_untouched() -> std::io::Result<()> {
  let installation = install()?;
  let function = "(scalar $value): string { return 'stripped'; }\n";
  write(&installation.project, "src/scalar.php", &format!("<?php\nfunction inProject{function}"))?;
  write(
    &installation.project,
    "vendor/acme/scalar.php",
    &format!("<?php\nfunction inVendor{function}"),
  )?;

  write(
    &installation.project,
    "main.php",
    r#"<?php
require __DIR__ . '/src/scalar.php';
require __DIR__ . '/vendor/acme/scalar.php';
echo inProject(1), "\n";
try {
    echo inVendor(1), "\n";
} catch (TypeError $error) {
    echo get_class($error), "\n";
}
"#,
  )?;

  let output = php(&installation, &["--strip-types", "main.php"])?;
  assert_eq!(text(&output.stderr), "");
  assert_eq!(text(&output.stdout), "stripped\nTypeError\n");
  return Ok(());
}

#[test]
fn strips_in_php_processes_started_by_the_script() -> std::io::Result<()> {
  let installation = install()?;
  write(
    &installation.project,
    "child.php",
    "<?php\nfunction twice(list<int> $items): int { return count($items) * 2; }\necho twice([1, 2]), \"\\n\";\n",
  )?;

  write(
    &installation.project,
    "main.php",
    "<?php\necho shell_exec(escapeshellarg(PHP_BINARY) . ' ' . escapeshellarg(__DIR__ . '/child.php'));\n",
  )?;

  let output = php(&installation, &["--strip-types", "main.php"])?;
  assert_eq!(text(&output.stderr), "");
  assert_eq!(text(&output.stdout), "4\n");
  return Ok(());
}

#[test]
fn reads_of_a_typed_file_return_its_original_text() -> std::io::Result<()> {
  let installation = install()?;
  write(
    &installation.project,
    "child.php",
    "<?php\nfunction twice(list<int> $items): int { return count($items) * 2; }\necho twice([1, 2]), \"\\n\";\n",
  )?;

  write(
    &installation.project,
    "main.php",
    r#"<?php
require __DIR__ . '/child.php';
$text = file_get_contents(__DIR__ . '/child.php');
echo str_contains($text, 'list<int> $items') ? 'original' : 'stripped', "\n";
"#,
  )?;

  let output = php(&installation, &["--strip-types", "main.php"])?;
  assert_eq!(text(&output.stderr), "");
  assert_eq!(text(&output.stdout), "4\noriginal\n");
  return Ok(());
}

#[test]
fn strips_an_edited_file_again_and_caches_each_version() -> std::io::Result<()> {
  let installation = install()?;
  let script = |factor: u32| {
    return format!(
      "<?php\nfunction scale(list<int> $items): int {{ return count($items) * {factor}; }}\necho scale([1, 2]), \"\\n\";\n"
    );
  };

  write(&installation.project, "main.php", &script(2))?;
  let first = php(&installation, &["--strip-types", "main.php"])?;
  write(&installation.project, "main.php", &script(3))?;
  let second = php(&installation, &["--strip-types", "main.php"])?;
  let cached =
    std::fs::read_dir(installation.home.join("cache"))?.collect::<Result<Vec<_>, _>>()?;

  assert_eq!(text(&first.stdout), "4\n");
  assert_eq!(text(&second.stdout), "6\n");
  assert_eq!(cached.len(), 2);
  return Ok(());
}

#[test]
fn writes_files_with_an_exclusive_lock() -> std::io::Result<()> {
  let installation = install()?;
  write(
    &installation.project,
    "main.php",
    "<?php\necho file_put_contents(__DIR__ . '/locked.txt', 'abc', LOCK_EX), \"\\n\";\n",
  )?;

  let output = php(&installation, &["--strip-types", "main.php"])?;
  assert_eq!(text(&output.stdout), "3\n");
  assert_eq!(text(&output.stderr), "");
  assert_eq!(std::fs::read_to_string(installation.project.join("locked.txt"))?, "abc");
  return Ok(());
}

#[test]
fn type_checks_the_project_through_the_shim() -> std::io::Result<()> {
  let installation = install()?;
  write(
    &installation.project,
    "mago.toml",
    "php-version = \"8.5.0\"\n\n[source]\npaths = [\"src\"]\n",
  )?;

  write(
    &installation.project,
    "src/total.php",
    "<?php\n\nfunction total(list<int> $numbers): int {\n    return count($numbers);\n}\n\necho total(['a']);\n",
  )?;

  let output = php(&installation, &["--typecheck"])?;
  assert_eq!(
    text(&output.stdout),
    "src/total.php:7: error[possibly-invalid-argument]: Possible argument type mismatch for argument #1 of `total`: expected `list<int>`, but possibly received `list{string('a')}`.\n"
  );

  assert_eq!(output.status.code(), Some(1));
  return Ok(());
}

#[test]
fn reports_a_missing_stripper_as_one_exception() -> std::io::Result<()> {
  let installation = install()?;
  let binary = installation.home.join("bin").join("typedhp");
  std::fs::remove_file(&binary)?;
  write(&installation.project, "main.php", "<?php\necho 'ran';\n")?;
  let output = php(&installation, &["--strip-types", "main.php"])?;
  let printed = [text(&output.stdout), text(&output.stderr)].concat();
  let expected = format!(
    "Uncaught RuntimeException: typedhp: {} is missing; run `typedhp install` again",
    binary.display()
  );

  assert!(printed.contains(&expected), "{printed}");
  assert!(!printed.contains("ran"), "{printed}");
  assert_eq!(output.status.code(), Some(255));
  return Ok(());
}
