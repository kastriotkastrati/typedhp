mod structure;

use crate::cli::Failure;
use crate::cli::write_output;
use itertools::Itertools;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::process::Stdio;
use structure::Entry;
use structure::EntryKind;
use structure::Issue;
use structure::Problem;
use structure::Report;
use typedhp::Desugared;

fn mirror_folder() -> PathBuf {
  return PathBuf::from(".typedhp").join("check");
}

fn generated_docblock_code() -> &'static str {
  return "redundant-docblock-type";
}

fn check_failure(path: &Path) -> impl FnOnce(std::io::Error) -> Failure {
  let path = path.to_path_buf();
  return move |error| Failure::Check { path, error };
}

fn project_entries(root: &Path) -> Result<Vec<Entry>, Failure> {
  let walker = ignore::WalkBuilder::new(root)
    .require_git(false)
    .filter_entry(|entry| {
      let is_directory = entry.file_type().is_some_and(|file_type| file_type.is_dir());
      let is_vendor = is_directory && entry.file_name() == "vendor";
      return !is_vendor;
    })
    .build();

  let walked = walker.collect::<Result<Vec<_>, _>>().map_err(|error| Failure::Walk { error })?;
  let entries = walked.iter().filter_map(|entry| {
    let relative = entry.path().strip_prefix(root).ok()?;
    let is_root = relative.as_os_str().is_empty();
    if is_root {
      return None;
    }

    let is_directory = entry.file_type().is_some_and(|file_type| file_type.is_dir());
    let is_php = relative.extension().is_some_and(|extension| extension == "php");
    let kind = match (is_directory, is_php) {
      (true, _) => EntryKind::Directory,
      (false, true) => EntryKind::Php,
      (false, false) => EntryKind::Other,
    };

    return Some(Entry { relative: relative.to_path_buf(), kind });
  });

  return Ok(entries.collect());
}

fn problem(path: &Path, line: usize, reason: impl Into<String>) -> Problem {
  return Problem { path: path.to_path_buf(), line, reason: reason.into() };
}

fn duplicate_aliases(aliases: &[(PathBuf, typedhp::TypeAlias)]) -> Vec<Problem> {
  let groups = aliases.iter().into_group_map_by(|(_, alias)| alias.name.to_ascii_lowercase());
  let repeated = groups.into_values().filter(|group| group.len() > 1);
  let problems = repeated.flat_map(|group| {
    let ordered = group
      .into_iter()
      .sorted_by_key(|(path, alias)| (path.clone(), alias.line))
      .collect::<Vec<_>>();

    let (first_path, first) = ordered[0];
    let first_place = format!("{}:{}", first_path.display(), first.line);
    return ordered.into_iter().skip(1).map(move |(path, alias)| {
      let name = String::from_utf8_lossy(&alias.name);
      return problem(
        path,
        alias.line,
        format!("type alias `{name}` is already declared at {first_place}"),
      );
    });
  });

  return problems.collect();
}

fn issue_line(
  issue: &Issue,
  mirrored: &HashMap<&PathBuf, &Desugared>,
) -> Option<(PathBuf, Option<usize>, bool)> {
  let primary = issue.annotations.iter().find(|annotation| annotation.kind == "Primary")?;
  let path = PathBuf::from(&primary.span.file_id.name);
  let start = &primary.span.start;
  let Some(desugared) = mirrored.get(&path) else {
    return Some((path, Some(start.line + 1), false));
  };

  let is_generated = desugared.docblocks.iter().any(|docblock| docblock.contains(start.offset));
  return Some((path, desugared.lines.get(start.line).copied(), is_generated));
}

fn issue_text(
  issue: &Issue,
  mirrored: &HashMap<&PathBuf, &Desugared>,
  unreadable: &HashSet<&PathBuf>,
) -> Option<String> {
  let level = issue.level.to_lowercase();
  let code = match &issue.code {
    Some(code) => format!("[{code}]"),
    None => String::new(),
  };

  let Some((path, line, is_generated)) = issue_line(issue, mirrored) else {
    return Some(format!("{level}{code}: {}\n", issue.message));
  };

  let flags_own_docblock = is_generated && issue.code.as_deref() == Some(generated_docblock_code());
  let is_in_unreadable_file = unreadable.contains(&path);
  if flags_own_docblock || is_in_unreadable_file {
    return None;
  }

  let place = match line {
    Some(line) => format!("{}:{line}", path.display()),
    None => path.display().to_string(),
  };

  return Some(format!("{place}: {level}{code}: {}\n", issue.message));
}

pub fn check_command(mago_arguments: &[OsString]) -> Result<ExitCode, Failure> {
  let root = std::env::current_dir().map_err(check_failure(Path::new(".")))?;
  let entries = project_entries(&root)?;
  let php_entries = entries.iter().filter(|entry| entry.kind == EntryKind::Php);
  let sources = php_entries
    .map(|entry| {
      let read = std::fs::read(root.join(&entry.relative));
      return read
        .map(|source| (entry.relative.clone(), source))
        .map_err(check_failure(&entry.relative));
    })
    .collect::<Result<Vec<_>, Failure>>()?;

  let alias_results =
    sources.iter().map(|(_, source)| typedhp::type_aliases(source)).collect::<Vec<_>>();

  let aliases = sources
    .iter()
    .zip(&alias_results)
    .flat_map(|((path, _), result)| {
      result.iter().flatten().map(|alias| (path.clone(), alias.clone()))
    })
    .collect::<Vec<_>>();

  let table = typedhp::alias_table(aliases.iter().map(|(_, alias)| alias));
  let desugared =
    sources.iter().map(|(_, source)| typedhp::desugar(source, &table)).collect::<Vec<_>>();

  let strip_errors =
    sources.iter().zip(alias_results.iter().zip(&desugared)).flat_map(|((path, _), results)| {
      let (alias_result, desugar_result) = results;
      let errors = [alias_result.as_ref().err(), desugar_result.as_ref().err()];
      return errors.into_iter().flatten().map(|error| problem(path, error.line, error.reason));
    });

  let problems =
    strip_errors.chain(duplicate_aliases(&aliases)).sorted().dedup().collect::<Vec<_>>();

  let mirrored = sources
    .iter()
    .zip(&desugared)
    .filter_map(|((path, _), result)| result.as_ref().ok().map(|desugared| (path, desugared)))
    .collect::<HashMap<_, _>>();

  let unreadable = sources
    .iter()
    .zip(&desugared)
    .filter_map(|((path, _), result)| result.is_err().then_some(path))
    .collect::<HashSet<_>>();

  let mirror = root.join(mirror_folder());
  let has_old_mirror = mirror.exists();
  if has_old_mirror {
    std::fs::remove_dir_all(&mirror).map_err(check_failure(&mirror))?;
  }

  std::fs::create_dir_all(&mirror).map_err(check_failure(&mirror))?;
  let ignore_file = root.join(".typedhp").join(".gitignore");
  std::fs::write(&ignore_file, "*\n").map_err(check_failure(&ignore_file))?;
  entries.iter().filter(|entry| entry.kind != EntryKind::Php).try_for_each(|entry| {
    let target = mirror.join(&entry.relative);
    let created = match entry.kind {
      EntryKind::Directory => std::fs::create_dir_all(&target),
      EntryKind::Php | EntryKind::Other => {
        std::os::unix::fs::symlink(root.join(&entry.relative), &target)
      }
    };

    return created.map_err(check_failure(&target));
  })?;

  sources.iter().zip(&desugared).try_for_each(|((path, source), result)| {
    let target = mirror.join(path);
    let code = match result {
      Ok(desugared) => desugared.code.as_slice(),
      Err(_) => source.as_slice(),
    };

    return std::fs::write(&target, code).map_err(check_failure(&target));
  })?;

  let vendor = root.join("vendor");
  let has_vendor = vendor.is_dir();
  if has_vendor {
    let linked_vendor = mirror.join("vendor");
    std::os::unix::fs::symlink(&vendor, &linked_vendor).map_err(check_failure(&linked_vendor))?;
  }

  let local_mago = vendor.join("bin").join("mago");
  // prelude-intentional-fallback: the project's own Mago wins; otherwise Mago comes from PATH
  let mago = if local_mago.is_file() { local_mago } else { PathBuf::from("mago") };
  let announcement =
    format!("typedhp: running {} analyze in {}\n", mago.display(), mirror_folder().display());

  let _ = std::io::stderr().write_all(announcement.as_bytes());
  let output = std::process::Command::new(&mago)
    .arg("analyze")
    .args(["--reporting-format", "json"])
    .args(mago_arguments)
    .current_dir(&mirror)
    .stdin(Stdio::null())
    .stderr(Stdio::inherit())
    .output()
    .map_err(|error| Failure::StartMago { program: mago.clone(), error })?;

  let mago_code = output.status.code().and_then(|code| u8::try_from(code).ok());
  let Some(mago_code) = mago_code else {
    return Err(Failure::MagoStopped { program: mago });
  };

  let report = serde_json::from_slice::<Report>(&output.stdout)
    .map_err(|error| Failure::MagoReport { error })?;

  let problem_lines = problems.iter().map(|problem| {
    return format!(
      "{}:{}: error[typedhp]: {}\n",
      problem.path.display(),
      problem.line,
      problem.reason
    );
  });

  let issue_lines =
    report.issues.iter().filter_map(|issue| issue_text(issue, &mirrored, &unreadable));

  let printed = problem_lines.chain(issue_lines).collect::<Vec<_>>();
  let has_output = !printed.is_empty();
  let summary = if has_output {
    format!("typedhp: found {} issues\n", printed.len())
  } else {
    "typedhp: no issues found\n".to_string()
  };

  write_output(printed.concat().as_bytes())?;
  let _ = std::io::stderr().write_all(summary.as_bytes());
  let has_problems = !problems.is_empty();
  let exit_code = if has_problems { mago_code.max(1) } else { mago_code };
  return Ok(ExitCode::from(exit_code));
}
