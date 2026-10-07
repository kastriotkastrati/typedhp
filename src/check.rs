mod structure;

pub use structure::CheckKind;
pub use structure::Problem;
pub use structure::Project;

use crate::cli::Failure;
use crate::cli::write_output;
use itertools::Either;
use itertools::Itertools;
use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::parser::parse_file_content;
use mago_text_edit::ApplyResult;
use mago_text_edit::TextEdit;
use mago_text_edit::TextEditor;
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
use structure::Report;
use structure::Safety;
use typedhp::ByteSpan;
use typedhp::Desugared;
use typedhp::title_case_types;

pub fn mirror_root() -> PathBuf {
  return PathBuf::from(".typedhp");
}

fn command_name(kind: CheckKind) -> &'static str {
  return match kind {
    CheckKind::Analyze => "analyze",
    CheckKind::Lint => "lint",
    CheckKind::Guard => "guard",
  };
}

fn mirror_name(kind: CheckKind) -> &'static str {
  return match kind {
    CheckKind::Analyze => "check",
    CheckKind::Lint => "lint",
    CheckKind::Guard => "guard",
  };
}

fn hides_generated_issue(kind: CheckKind, code: Option<&str>) -> bool {
  return match kind {
    CheckKind::Analyze => code == Some("redundant-docblock-type"),
    CheckKind::Lint | CheckKind::Guard => true,
  };
}

fn unsupported_check_flags() -> [&'static str; 8] {
  return [
    "--generate-baseline",
    "--remove-outdated-baseline-entries",
    "--staged",
    "--stdin-input",
    "--substitute",
    "--format-after-fix",
    "--dry-run",
    "-d",
  ];
}

fn reporting_options() -> [&'static str; 2] {
  return ["--reporting-format", "--reporting-target"];
}

fn fix_flags() -> [&'static str; 4] {
  return ["--fix", "--unsafe", "--potentially-unsafe", "--fail-on-remaining"];
}

fn report_level_option() -> &'static str {
  return "--minimum-report-level";
}

fn mago_variable() -> &'static str {
  return "TYPEDHP_MAGO";
}

pub fn check_failure(path: &Path) -> impl FnOnce(std::io::Error) -> Failure {
  let path = path.to_path_buf();
  return move |error| Failure::Check { path, error };
}

pub fn matches_flag(argument: &OsString, flag: &str) -> bool {
  let text = argument.to_string_lossy();
  let is_joined = text.strip_prefix(flag).is_some_and(|rest| rest.starts_with('='));
  return text == flag || is_joined;
}

pub fn mirrored_argument(argument: &OsString, root: &Path, mirror: &Path) -> OsString {
  let path = Path::new(argument);
  return match path.strip_prefix(root) {
    Ok(relative) => mirror.join(relative).into_os_string(),
    Err(_) => argument.clone(),
  };
}

pub fn problem_line(problem: &Problem) -> String {
  return format!(
    "{}:{}: error[typedhp]: {}\n",
    problem.path.display(),
    problem.line,
    problem.reason
  );
}

pub fn real_mago(root: &Path) -> Result<PathBuf, Failure> {
  // prelude-intentional-fallback: TYPEDHP_MAGO names Mago outright; otherwise the project's own Mago wins, then Mago from PATH
  let configured = std::env::var_os(mago_variable());
  if let Some(configured) = configured {
    return Ok(PathBuf::from(configured));
  }

  let local = root.join("vendor").join("bin").join("mago");
  let has_local = local.is_file();
  if has_local {
    return Ok(local);
  }

  let executable = std::env::current_exe().map_err(check_failure(Path::new("typedhp")))?;
  let own_folder = executable.parent().and_then(|folder| folder.canonicalize().ok());
  let Some(search_path) = std::env::var_os("PATH") else {
    return Err(Failure::MissingMago);
  };

  let candidates = std::env::split_paths(&search_path).map(|folder| folder.join("mago"));
  let found = candidates.filter(|candidate| candidate.is_file()).find(|candidate| {
    let canonical = candidate.canonicalize().ok();
    let folder = canonical.as_deref().and_then(Path::parent);
    let is_shim = folder.is_some() && folder == own_folder.as_deref();
    return !is_shim;
  });

  return found.ok_or(Failure::MissingMago);
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

pub fn read_project(root: &Path) -> Result<Project, Failure> {
  let entries = project_entries(root)?;
  let php_entries = entries.iter().filter(|entry| entry.kind == EntryKind::Php);
  let sources = php_entries
    .map(|entry| {
      let read = std::fs::read(root.join(&entry.relative));
      return read
        .map(|source| (entry.relative.clone(), source))
        .map_err(check_failure(&entry.relative));
    })
    .collect::<Result<Vec<_>, Failure>>()?;

  return Ok(Project { root: root.to_path_buf(), entries, sources });
}

pub fn is_typed(project: &Project) -> bool {
  return project.sources.iter().any(|(_, source)| {
    let stripped = typedhp::strip(source);
    let is_plain = stripped.is_ok_and(|code| code == *source);
    return !is_plain;
  });
}

pub fn write_mirror(project: &Project, codes: &[&[u8]], folder: &Path) -> Result<PathBuf, Failure> {
  let root = &project.root;
  let mirror = root.join(folder);
  let has_old_mirror = mirror.exists();
  if has_old_mirror {
    std::fs::remove_dir_all(&mirror).map_err(check_failure(&mirror))?;
  }

  std::fs::create_dir_all(&mirror).map_err(check_failure(&mirror))?;
  let ignore_file = root.join(mirror_root()).join(".gitignore");
  std::fs::write(&ignore_file, "*\n").map_err(check_failure(&ignore_file))?;
  let others = project.entries.iter().filter(|entry| entry.kind != EntryKind::Php);
  others.clone().try_for_each(|entry| {
    let target = mirror.join(&entry.relative);
    let created = match entry.kind {
      EntryKind::Directory => std::fs::create_dir_all(&target),
      EntryKind::Php | EntryKind::Other => {
        std::os::unix::fs::symlink(root.join(&entry.relative), &target)
      }
    };

    return created.map_err(check_failure(&target));
  })?;

  project.sources.iter().zip(codes).try_for_each(|((path, _), code)| {
    let target = mirror.join(path);
    return std::fs::write(&target, code).map_err(check_failure(&target));
  })?;

  let directories = others.filter(|entry| entry.kind == EntryKind::Directory);
  let folders =
    std::iter::once(PathBuf::new()).chain(directories.map(|entry| entry.relative.clone()));

  let mut vendors =
    folders.map(|folder| folder.join("vendor")).filter(|vendor| root.join(vendor).is_dir());

  vendors.try_for_each(|vendor| {
    let target = mirror.join(&vendor);
    return std::os::unix::fs::symlink(root.join(&vendor), &target).map_err(check_failure(&target));
  })?;

  return Ok(mirror);
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

fn issue_prefix(
  kind: CheckKind,
  issue: &Issue,
  mirrored: &HashMap<&PathBuf, &Desugared>,
  unreadable: &HashSet<&PathBuf>,
) -> Option<String> {
  let primary = issue.annotations.iter().find(|annotation| annotation.kind == "Primary");
  let Some(primary) = primary else {
    return Some(String::new());
  };

  let path = PathBuf::from(&primary.span.file_id.name);
  let start = &primary.span.start;
  let desugared = mirrored.get(&path);
  let is_generated = desugared.is_some_and(|desugared| {
    return desugared.docblocks.iter().any(|docblock| docblock.contains(start.offset));
  });

  let is_hidden = is_generated && hides_generated_issue(kind, issue.code.as_deref());
  let is_in_unreadable_file = unreadable.contains(&path);
  if is_hidden || is_in_unreadable_file {
    return None;
  }

  let line = match desugared {
    Some(desugared) => desugared.lines.get(start.line).copied(),
    None => Some(start.line + 1),
  };

  return match line {
    Some(line) => Some(format!("{}:{line}: ", path.display())),
    None => Some(format!("{}: ", path.display())),
  };
}

fn fix_issues(
  project: &Project,
  arguments: &[OsString],
  problems: &[Problem],
  shown: &[(String, &Issue)],
  mirrored: &HashMap<&PathBuf, &Desugared>,
) -> Result<ExitCode, Failure> {
  let has_flag = |flag: &str| arguments.iter().any(|argument| matches_flag(argument, flag));
  let allows_unsafe = has_flag("--unsafe");
  let allows_potentially_unsafe = has_flag("--potentially-unsafe");
  let threshold = if allows_unsafe {
    Safety::Unsafe
  } else if allows_potentially_unsafe {
    Safety::PotentiallyUnsafe
  } else {
    Safety::Safe
  };

  let skipped = |prefix: &str, issue: &Issue, reason: &str| {
    let subject = match &issue.code {
      Some(code) => format!("the `{code}` fix"),
      None => "a fix".to_string(),
    };

    return format!("{prefix}error[typedhp]: skipped {subject}, because {reason}\n");
  };

  let batches = shown.iter().flat_map(|(prefix, issue)| {
    return issue.edits.iter().map(move |(file, changes)| (prefix.as_str(), *issue, file, changes));
  });

  let (allowed, too_risky): (Vec<_>, Vec<_>) = batches.partition_map(|batch| {
    let (_, _, _, changes) = batch;
    let exceeding = changes.iter().map(|change| change.safety).find(|safety| *safety > threshold);
    return match exceeding {
      Some(safety) => Either::Right(safety),
      None => Either::Left(batch),
    };
  });

  let (mapped, unmapped): (Vec<_>, Vec<_>) =
    allowed.into_iter().partition_map(|(prefix, issue, file, changes)| {
      let path = PathBuf::from(&file.name);
      let desugared = mirrored.get(&path);
      let edits = changes.iter().map(|change| {
        let span = ByteSpan { start: change.range.start, end: change.range.end };
        let source_span = typedhp::source_span(desugared?, span)?;
        let start = u32::try_from(source_span.start).ok()?;
        let end = u32::try_from(source_span.end).ok()?;
        return Some(TextEdit::replace(start..end, change.new_text.clone()));
      });

      return match edits.collect::<Option<Vec<_>>>() {
        Some(edits) => Either::Left((path, prefix, issue, edits)),
        None => Either::Right(skipped(prefix, issue, "it changes code that typedhp rewrote")),
      };
    });

  let still_parses = |code: &[u8]| {
    let Ok(stripped) = typedhp::strip(code) else {
      return false;
    };

    let arena = LocalArena::new();
    let program = parse_file_content(&arena, FileId::zero(), &stripped);
    return !program.has_errors();
  };

  let by_file = mapped.into_iter().into_group_map_by(|(path, ..)| path.clone());
  let fixed_files = project
    .sources
    .iter()
    .filter_map(|(path, source)| {
      let file_batches = by_file.get(path)?;
      let mut editor = TextEditor::new(source);
      let results = file_batches.iter().map(|(_, prefix, issue, edits)| {
        let result = editor.apply_batch(edits.clone(), Some(&still_parses));
        return (*prefix, *issue, result);
      });

      let results = results.collect::<Vec<_>>();
      return Some((path, source, editor.finish(), results));
    })
    .collect::<Vec<_>>();

  let changed =
    fixed_files.iter().filter(|(_, source, fixed, _)| fixed != *source).collect::<Vec<_>>();

  changed.iter().try_for_each(|(path, _, fixed, _)| {
    let target = project.root.join(path);
    return std::fs::write(&target, fixed).map_err(check_failure(&target));
  })?;

  let results = fixed_files.iter().flat_map(|(.., results)| results).collect::<Vec<_>>();
  let overlapping = results.iter().filter(|(_, _, result)| *result == ApplyResult::Overlap).count();
  let failures = results.iter().filter_map(|(prefix, issue, result)| {
    let reason = match result {
      ApplyResult::Applied | ApplyResult::Overlap => return None,
      ApplyResult::Rejected => "the fixed file would not parse",
      _ => "Mago gave an edit that does not fit the file",
    };

    return Some(skipped(prefix, issue, reason));
  });

  let messages = unmapped.into_iter().chain(failures).sorted().collect::<Vec<_>>();
  let printed = problems.iter().map(problem_line).chain(messages.iter().cloned());
  write_output(printed.collect::<String>().as_bytes())?;
  let skipped_unsafe = too_risky.iter().filter(|safety| **safety == Safety::Unsafe).count();
  let skipped_potentially_unsafe =
    too_risky.iter().filter(|safety| **safety == Safety::PotentiallyUnsafe).count();

  let unfixable = shown.iter().filter(|(_, issue)| issue.edits.is_empty()).count();
  let remaining = unfixable + too_risky.len();
  let has_skipped_unsafe = skipped_unsafe > 0;
  let has_skipped_potentially_unsafe = skipped_potentially_unsafe > 0;
  let has_overlapping = overlapping > 0;
  let has_fixed = !changed.is_empty();
  let fails_on_remaining = has_flag("--fail-on-remaining") && remaining > 0;
  let fixed_note = if has_fixed {
    format!("typedhp: fixed {} files\n", changed.len())
  } else {
    "typedhp: no fixes were applied\n".to_string()
  };

  let notes = [
    has_skipped_unsafe.then(|| {
      format!("typedhp: skipped {skipped_unsafe} unsafe fixes; use `--unsafe` to apply them\n")
    }),
    has_skipped_potentially_unsafe.then(|| {
      format!(
        "typedhp: skipped {skipped_potentially_unsafe} potentially unsafe fixes; use `--potentially-unsafe` or `--unsafe` to apply them\n"
      )
    }),
    has_overlapping.then(|| {
      format!(
        "typedhp: skipped {overlapping} fixes that overlap other fixes; run the command again to apply them\n"
      )
    }),
    Some(fixed_note),
    fails_on_remaining.then(|| format!("typedhp: {remaining} issues need fixing by hand\n")),
  ];

  let _ = std::io::stderr().write_all(notes.into_iter().flatten().collect::<String>().as_bytes());
  let has_errors = !problems.is_empty() || !messages.is_empty();
  return Ok(ExitCode::from(u8::from(has_errors || fails_on_remaining)));
}

pub fn check_project(
  project: &Project,
  mago: &Path,
  kind: CheckKind,
  globals: &[OsString],
  arguments: &[OsString],
) -> Result<ExitCode, Failure> {
  let command = command_name(kind);
  let is_fixing = arguments.iter().any(|argument| matches_flag(argument, "--fix"));
  let unsupported = arguments.iter().find(|argument| {
    return unsupported_check_flags().iter().any(|flag| matches_flag(argument, flag));
  });

  if let Some(flag) = unsupported {
    return Err(Failure::Unsupported { command, flag: flag.clone() });
  }

  let sources = &project.sources;
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

  let codes = sources
    .iter()
    .zip(&desugared)
    .map(|((_, source), result)| {
      return match result {
        Ok(desugared) => desugared.code.as_slice(),
        Err(_) => source.as_slice(),
      };
    })
    .collect::<Vec<_>>();

  let folder = mirror_root().join(mirror_name(kind));
  let mirror = write_mirror(project, &codes, &folder)?;
  let level_options = [report_level_option()].into_iter().filter(|_| is_fixing);
  let valued_options = reporting_options().into_iter().chain(level_options).collect::<Vec<_>>();
  let fix_switches = fix_flags().into_iter().filter(|_| is_fixing).collect::<Vec<_>>();
  let kept_arguments = arguments.iter().enumerate().filter(|(index, argument)| {
    let is_valued_option = valued_options.iter().any(|option| matches_flag(argument, option));
    let is_fix_switch = fix_switches.iter().any(|switch| matches_flag(argument, switch));
    let follows_valued_option = index.checked_sub(1).is_some_and(|previous| {
      let previous_argument = arguments[previous].to_string_lossy();
      return valued_options.contains(&previous_argument.as_ref());
    });

    return !is_valued_option && !is_fix_switch && !follows_valued_option;
  });

  let mirrored_arguments = kept_arguments
    .map(|(_, argument)| mirrored_argument(argument, &project.root, &mirror))
    .collect::<Vec<_>>();

  let every_level = [report_level_option(), "note"].into_iter().filter(|_| is_fixing);
  let announcement = format!("typedhp: running mago {command} in {}\n", folder.display());
  let _ = std::io::stderr().write_all(announcement.as_bytes());
  let output = std::process::Command::new(mago)
    .args(globals)
    .arg(command)
    .args(["--reporting-format", "json"])
    .args(every_level)
    .args(&mirrored_arguments)
    .current_dir(&mirror)
    .stdin(Stdio::null())
    .stderr(Stdio::inherit())
    .output()
    .map_err(|error| Failure::StartMago { program: mago.to_path_buf(), error })?;

  let mago_code = output.status.code().and_then(|code| u8::try_from(code).ok());
  let Some(mago_code) = mago_code else {
    return Err(Failure::MagoStopped { program: mago.to_path_buf() });
  };

  let report = serde_json::from_slice::<Report>(&output.stdout)
    .map_err(|error| Failure::MagoReport { error })?;

  let shown = report
    .issues
    .iter()
    .filter_map(|issue| {
      let prefix = issue_prefix(kind, issue, &mirrored, &unreadable)?;
      return Some((prefix, issue));
    })
    .collect::<Vec<_>>();

  if is_fixing {
    return fix_issues(project, arguments, &problems, &shown, &mirrored);
  }

  let problem_lines = problems.iter().map(problem_line);
  let issue_lines = shown.iter().map(|(prefix, issue)| {
    let level = issue.level.to_lowercase();
    let code = match &issue.code {
      Some(code) => format!("[{code}]"),
      None => String::new(),
    };

    return format!("{prefix}{level}{code}: {}\n", title_case_types(&issue.message));
  });

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

pub fn check_command(mago_arguments: &[OsString]) -> Result<ExitCode, Failure> {
  let root = std::env::current_dir().map_err(check_failure(Path::new(".")))?;
  let project = read_project(&root)?;
  let mago = real_mago(&root)?;
  return check_project(&project, &mago, CheckKind::Analyze, &[], mago_arguments);
}
