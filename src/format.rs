use crate::check::Problem;
use crate::check::Project;
use crate::check::check_failure;
use crate::check::matches_flag;
use crate::check::mirror_root;
use crate::check::mirrored_argument;
use crate::check::problem_line;
use crate::check::write_mirror;
use crate::cli::Failure;
use crate::cli::write_output;
use itertools::Itertools;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::process::Stdio;

fn format_folder() -> PathBuf {
  return mirror_root().join("format");
}

fn unsupported_format_flags() -> [&'static str; 6] {
  return ["--dry-run", "-d", "--staged", "-s", "--stdin-input", "-i"];
}

pub fn format_project(
  project: &Project,
  mago: &Path,
  globals: &[OsString],
  arguments: &[OsString],
) -> Result<ExitCode, Failure> {
  let unsupported = arguments.iter().find(|argument| {
    return unsupported_format_flags().iter().any(|flag| matches_flag(argument, flag));
  });

  if let Some(flag) = unsupported {
    return Err(Failure::Unsupported { command: "format", flag: flag.clone() });
  }

  let sources = &project.sources;
  let masked = sources.iter().map(|(_, source)| typedhp::mask(source)).collect_vec();
  let mask_problems = sources.iter().zip(&masked).filter_map(|((path, _), result)| {
    let error = result.as_ref().err()?;
    return Some(Problem {
      path: path.clone(),
      line: error.line,
      reason: error.reason.to_string(),
    });
  });

  let codes = sources
    .iter()
    .zip(&masked)
    .map(|((_, source), result)| {
      return match result {
        Ok(masked) => masked.code.as_slice(),
        Err(_) => source.as_slice(),
      };
    })
    .collect_vec();

  let folder = format_folder();
  let mirror = write_mirror(project, &codes, &folder)?;
  let mirrored_arguments =
    arguments.iter().map(|argument| mirrored_argument(argument, &project.root, &mirror));

  let announcement = format!("typedhp: running mago format in {}\n", folder.display());
  let _ = std::io::stderr().write_all(announcement.as_bytes());
  let status = std::process::Command::new(mago)
    .args(globals)
    .arg("format")
    .args(mirrored_arguments)
    .current_dir(&mirror)
    .stdin(Stdio::null())
    .status()
    .map_err(|error| Failure::StartMago { program: mago.to_path_buf(), error })?;

  let mago_code = status.code().and_then(|code| u8::try_from(code).ok());
  let Some(mago_code) = mago_code else {
    return Err(Failure::MagoStopped { program: mago.to_path_buf() });
  };

  let masked_sources = sources.iter().zip(&masked).filter_map(|((path, source), result)| {
    return result.as_ref().ok().map(|masked| (path, source, masked));
  });

  let formatted = masked_sources
    .map(|(path, source, masked)| {
      let mirrored = mirror.join(path);
      let output = std::fs::read(&mirrored).map_err(check_failure(&mirrored))?;
      let is_untouched = output == masked.code;
      if is_untouched {
        return Ok(None);
      }

      return Ok(Some((path, source, typedhp::unmask(&output, masked))));
    })
    .collect::<Result<Vec<_>, Failure>>()?;

  let restored = formatted.into_iter().flatten();
  let (rewrites, unmask_errors): (Vec<_>, Vec<_>) =
    restored.partition_map(|(path, source, result)| {
      return match result {
        Ok(code) => itertools::Either::Left((path, source, code)),
        Err(error) => itertools::Either::Right(Problem {
          path: path.clone(),
          line: error.line,
          reason: error.reason.to_string(),
        }),
      };
    });

  let changed = rewrites.iter().filter(|(_, source, code)| code != *source);
  changed.into_iter().try_for_each(|(path, _, code)| {
    let target = project.root.join(path);
    return std::fs::write(&target, code).map_err(check_failure(&target));
  })?;

  let problems = mask_problems.chain(unmask_errors).sorted().collect_vec();
  write_output(problems.iter().map(problem_line).collect::<String>().as_bytes())?;
  let has_problems = !problems.is_empty();
  let exit_code = if has_problems { mago_code.max(1) } else { mago_code };
  return Ok(ExitCode::from(exit_code));
}
