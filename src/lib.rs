//! sshpass for Windows: runs a command inside a pseudo console (`ConPTY`) and
//! types the password when the command asks for it.

#[cfg(not(windows))]
compile_error!("sshpass drives the Windows pseudo console (ConPTY) and only builds for Windows");

pub mod cli;
pub mod cmdline;
mod conpty;
mod console;
pub mod error;
pub mod host_modes;
pub mod password;
pub mod prompt;
mod session;
pub mod utf16;
mod win32;

use std::ffi::OsString;
use std::io::{self, Write};
use std::iter;

pub use crate::error::{Error, Status};

use crate::cli::{Invocation, Usage};
use crate::cmdline::CommandLine;
use crate::prompt::Detector;
use crate::session::Outcome;

/// Runs sshpass on `args` (without the program name) and returns the exit
/// code: the command's own, or a [`Status`] if sshpass itself gave up.
pub fn run(args: impl IntoIterator<Item = OsString>) -> u32 {
    execute(args).unwrap_or_else(|error| {
        report(&error);
        error.status().into()
    })
}

fn execute(args: impl IntoIterator<Item = OsString>) -> Result<u32, Error> {
    let options = match cli::parse(args)? {
        Invocation::Help => {
            write!(io::stdout(), "{Usage}").map_err(Error::io("print help"))?;
            return Ok(0);
        }
        Invocation::Run(options) => options,
    };

    let password = options.password.resolve()?;
    let command_line = CommandLine::new(&options.program, &options.args);
    if options.verbose {
        eprintln!(
            "SSHPASS: searching for password prompt using match \"{}\"",
            options.prompt
        );
        eprintln!("SSHPASS: cmd: {command_line}");
    }

    let detector = Detector::new(&options.prompt, options.auto_confirm);
    match session::run(command_line, &password, detector, options.verbose)? {
        Outcome::Exited(code) => Ok(code),
        Outcome::Rejected(failure) => Err(Error::Rejected(failure)),
    }
}

fn report(error: &Error) {
    let message = iter::successors(Some(error as &dyn std::error::Error), |&error| {
        error.source()
    })
    .map(ToString::to_string)
    .collect::<Vec<_>>()
    .join(": ");
    eprintln!("sshpass: {message}");
    if matches!(error, Error::Usage(_)) {
        eprint!("\n{Usage}");
    }
}
