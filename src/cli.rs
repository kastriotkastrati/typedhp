mod structure;

pub use structure::Failure;

use crate::check::check_command;
use std::ffi::OsString;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use structure::Command;
use structure::StripInput;

fn usage() -> &'static str {
  return "usage:
  typedhp strip <file>          print <file> with its types stripped
  typedhp strip --stdin <name>  strip PHP read from stdin; <name> labels errors
  typedhp install <folder>      install typedhp, the php shim and the loader into <folder>
  typedhp check [<mago args>]   type-check the project in this folder with Mago, through .typedhp/check/
";
}

fn strip_error_exit_code() -> u8 {
  return 1;
}

fn failure_exit_code() -> u8 {
  return 2;
}

fn shim() -> &'static str {
  return include_str!("../runtime/php");
}

fn loader_files() -> [(&'static str, &'static str); 4] {
  return [
    ("loader.php", include_str!("../runtime/loader.php")),
    ("Typedhp/ProcessRun.php", include_str!("../runtime/Typedhp/ProcessRun.php")),
    ("Typedhp/Stripper.php", include_str!("../runtime/Typedhp/Stripper.php")),
    (
      "Typedhp/StrippingFileWrapper.php",
      include_str!("../runtime/Typedhp/StrippingFileWrapper.php"),
    ),
  ];
}

fn ini_unsafe_characters() -> [char; 5] {
  return ['"', '$', '\\', '\n', '\r'];
}

pub fn run() -> ExitCode {
  let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
  let outcome = parse(&arguments).and_then(|command| execute(&command));
  return match outcome {
    Ok(code) => code,
    Err(failure) => report(&failure),
  };
}

fn parse(arguments: &[OsString]) -> Result<Command, Failure> {
  return match arguments {
    [command, flag, name] if command == "strip" && flag == "--stdin" => {
      Ok(Command::Strip { path: PathBuf::from(name), input: StripInput::Stdin })
    }
    [command, path] if command == "strip" => {
      Ok(Command::Strip { path: PathBuf::from(path), input: StripInput::File })
    }
    [command, folder] if command == "install" => {
      Ok(Command::Install { folder: PathBuf::from(folder) })
    }
    [command, mago_arguments @ ..] if command == "check" => {
      Ok(Command::Check { mago_arguments: mago_arguments.to_vec() })
    }
    _ => Err(Failure::Usage),
  };
}

fn execute(command: &Command) -> Result<ExitCode, Failure> {
  return match command {
    Command::Strip { path, input } => strip_command(path, *input).map(|()| ExitCode::SUCCESS),
    Command::Install { folder } => install_command(folder).map(|()| ExitCode::SUCCESS),
    Command::Check { mago_arguments } => check_command(mago_arguments),
  };
}

fn strip_command(path: &Path, input: StripInput) -> Result<(), Failure> {
  let source = match input {
    StripInput::File => std::fs::read(path),
    StripInput::Stdin => std::io::stdin().lock().bytes().collect::<Result<Vec<u8>, _>>(),
  };

  let source = source.map_err(|error| Failure::Read { path: path.to_path_buf(), error })?;
  let stripped =
    typedhp::strip(&source).map_err(|error| Failure::Strip { path: path.to_path_buf(), error })?;

  return write_output(&stripped);
}

fn install_command(folder: &Path) -> Result<(), Failure> {
  let stripped_loader_files = loader_files()
    .into_iter()
    .map(|(relative, contents)| {
      let stripped = typedhp::strip(contents.as_bytes());
      return stripped
        .map(|code| (relative, code))
        .map_err(|error| Failure::Strip { path: PathBuf::from(relative), error });
    })
    .collect::<Result<Vec<_>, Failure>>()?;

  let executable = std::env::current_exe().map_err(install_failure(Path::new("typedhp")))?;
  let binary = std::fs::read(&executable).map_err(install_failure(&executable))?;
  std::fs::create_dir_all(folder).map_err(install_failure(folder))?;
  let home = folder.canonicalize().map_err(install_failure(folder))?;
  let home_text = home.to_str().ok_or(Failure::InstallFolder {
    folder: home.clone(),
    reason: "the folder path is not valid UTF-8",
  })?;

  let is_ini_safe = !home_text.contains(ini_unsafe_characters());
  if !is_ini_safe {
    return Err(Failure::InstallFolder {
      folder: home.clone(),
      reason: "the folder path cannot contain \", $, \\ or line breaks, because php.ini reads it",
    });
  }

  let bin = home.join("bin");
  let ini = home.join("ini");
  let cache = home.join("cache");
  let classes = home.join("Typedhp");
  [&bin, &ini, &cache, &classes].into_iter().try_for_each(|directory| {
    return std::fs::create_dir_all(directory).map_err(install_failure(directory));
  })?;

  let private = std::fs::Permissions::from_mode(0o700);
  std::fs::set_permissions(&cache, private).map_err(install_failure(&cache))?;
  let ini_text = format!("auto_prepend_file = \"{home_text}/loader.php\"\n");
  write_file(&bin.join("typedhp"), &binary, 0o755)?;
  write_file(&bin.join("php"), shim().as_bytes(), 0o755)?;
  stripped_loader_files.iter().try_for_each(|(relative, code)| {
    return write_file(&home.join(relative), code, 0o644);
  })?;

  write_file(&ini.join("typedhp.ini"), ini_text.as_bytes(), 0o644)?;
  let summary = format!(
    "installed typedhp in {home_text}
  {home_text}/bin/php          adds --strip-types and --typecheck to php
  {home_text}/bin/typedhp      strips and checks types
  {home_text}/loader.php       strips each file PHP includes, except files under vendor/
  {home_text}/Typedhp/         the classes loader.php uses
  {home_text}/ini/typedhp.ini  makes PHP run loader.php first, through auto_prepend_file
  {home_text}/cache/           stripped files, named by the hash of their source

Put {home_text}/bin first on PATH, so that `php` finds the shim:
  with mise:  add  _.path = [\"{home_text}/bin\"]  under [env] in ~/.config/mise/config.toml
  otherwise:  export PATH=\"{home_text}/bin:$PATH\"
"
  );

  return write_output(summary.as_bytes());
}

fn install_failure(path: &Path) -> impl FnOnce(std::io::Error) -> Failure {
  let path = path.to_path_buf();
  return move |error| Failure::Install { path, error };
}

fn write_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), Failure> {
  let temporary = path.with_extension("typedhp-install");
  let permissions = std::fs::Permissions::from_mode(mode);
  let written = std::fs::write(&temporary, contents)
    .and_then(|()| std::fs::set_permissions(&temporary, permissions))
    .and_then(|()| std::fs::rename(&temporary, path));

  return written.map_err(install_failure(path));
}

pub fn write_output(bytes: &[u8]) -> Result<(), Failure> {
  let written = std::io::stdout().write_all(bytes).and_then(|()| std::io::stdout().flush());
  return written.map_err(|error| Failure::WriteOutput { error });
}

fn report(failure: &Failure) -> ExitCode {
  let (message, code) = match failure {
    Failure::Usage => (usage().to_string(), failure_exit_code()),
    Failure::Read { path, error } => {
      (format!("{}: {error}\n", path.display()), failure_exit_code())
    }
    Failure::Strip { path, error } => {
      let message = format!("{}:{}: {}\n", path.display(), error.line, error.reason);
      (message, strip_error_exit_code())
    }
    Failure::WriteOutput { error } => {
      (format!("cannot write output: {error}\n"), failure_exit_code())
    }
    Failure::InstallFolder { folder, reason } => {
      (format!("{}: {reason}\n", folder.display()), failure_exit_code())
    }
    Failure::Install { path, error } | Failure::Check { path, error } => {
      (format!("{}: {error}\n", path.display()), failure_exit_code())
    }
    Failure::Walk { error } => {
      (format!("cannot list the project files: {error}\n"), failure_exit_code())
    }
    Failure::StartMago { program, error } => {
      let message = format!(
        "cannot run {}: {error}\ninstall Mago with `composer require --dev carthage-software/mago`, or put `mago` on PATH\n",
        program.display()
      );

      (message, failure_exit_code())
    }
    Failure::MagoStopped { program } => {
      (format!("{} stopped without an exit code\n", program.display()), failure_exit_code())
    }
    Failure::MagoReport { error } => {
      (format!("cannot read the JSON report from Mago: {error}\n"), failure_exit_code())
    }
  };

  let _ = std::io::stderr().write_all(message.as_bytes());
  return ExitCode::from(code);
}
