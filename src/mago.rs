mod structure;

use crate::check::CheckKind;
use crate::check::check_failure;
use crate::check::check_project;
use crate::check::is_typed;
use crate::check::matches_flag;
use crate::check::read_project;
use crate::check::real_mago;
use crate::cli::Failure;
use crate::format::format_project;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use structure::Invocation;
use structure::TypedCommand;

fn options_with_values() -> [&'static str; 5] {
  return ["--workspace", "--config", "--php-version", "--threads", "--colors"];
}

fn workspace_option() -> &'static str {
  return "--workspace";
}

fn takes_value(argument: &OsString) -> bool {
  return options_with_values().iter().any(|option| argument.to_str() == Some(option));
}

fn invocation(arguments: &[OsString]) -> Invocation {
  let command_index = arguments.iter().enumerate().position(|(index, argument)| {
    let is_option = argument.to_string_lossy().starts_with('-');
    let is_value = index.checked_sub(1).is_some_and(|previous| takes_value(&arguments[previous]));
    return !is_option && !is_value;
  });

  let globals_end = command_index.unwrap_or(arguments.len());
  let globals = &arguments[..globals_end];
  let mut workspace_values = globals.iter().enumerate().filter_map(|(index, argument)| {
    let text = argument.to_string_lossy();
    let joined = text.strip_prefix(workspace_option()).and_then(|rest| rest.strip_prefix('='));
    if let Some(joined) = joined {
      return Some(PathBuf::from(joined));
    }

    let is_workspace = text == workspace_option();
    let value = globals.get(index + 1).filter(|_| is_workspace);
    return value.map(PathBuf::from);
  });

  let workspace = workspace_values.next_back();
  let kept_globals = globals.iter().enumerate().filter(|(index, argument)| {
    let is_workspace = matches_flag(argument, workspace_option());
    let follows_workspace = index.checked_sub(1).is_some_and(|previous| {
      return globals[previous].to_str() == Some(workspace_option());
    });

    return !is_workspace && !follows_workspace;
  });

  let command = command_index.map(|index| arguments[index].to_string_lossy().into_owned());
  let rest = command_index.map_or(&[][..], |index| &arguments[index + 1..]);
  return Invocation {
    globals: kept_globals.map(|(_, argument)| argument.clone()).collect(),
    workspace,
    command,
    arguments: rest.to_vec(),
  };
}

fn run_unchanged(mago: &Path, arguments: &[OsString]) -> Failure {
  let error = std::process::Command::new(mago).args(arguments).exec();
  return Failure::StartMago { program: mago.to_path_buf(), error };
}

pub fn mago_command(arguments: &[OsString]) -> Result<ExitCode, Failure> {
  let invocation = invocation(arguments);
  let current = std::env::current_dir().map_err(check_failure(Path::new(".")))?;
  let root = match &invocation.workspace {
    Some(workspace) => current.join(workspace),
    None => current,
  };

  let mago = real_mago(&root)?;
  let typed_command = match invocation.command.as_deref() {
    Some("format" | "fmt") => Some(TypedCommand::Format),
    Some("analyze" | "analyse") => Some(TypedCommand::Check(CheckKind::Analyze)),
    Some("lint") => Some(TypedCommand::Check(CheckKind::Lint)),
    Some("guard") => Some(TypedCommand::Check(CheckKind::Guard)),
    _ => None,
  };

  let Some(typed_command) = typed_command else {
    return Err(run_unchanged(&mago, arguments));
  };

  let project = read_project(&root)?;
  let has_typed_files = is_typed(&project);
  if !has_typed_files {
    return Err(run_unchanged(&mago, arguments));
  }

  let globals = &invocation.globals;
  let rest = &invocation.arguments;
  return match typed_command {
    TypedCommand::Format => format_project(&project, &mago, globals, rest),
    TypedCommand::Check(kind) => check_project(&project, &mago, kind, globals, rest),
  };
}
