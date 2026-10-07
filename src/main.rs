#![allow(clippy::needless_return, clippy::let_and_return)]

mod check;
mod cli;
mod format;
mod mago;

fn main() -> std::process::ExitCode {
  return cli::run();
}
