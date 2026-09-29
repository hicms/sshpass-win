//! Where the password comes from, and a buffer that wipes it.

use std::env::{self, VarError};
use std::fs::File;
use std::io::{self, Read};
use std::num::NonZeroUsize;
use std::os::windows::io::{AsHandle, BorrowedHandle};
use std::path::PathBuf;
use std::sync::atomic::{Ordering, compiler_fence};
use std::{fmt, mem, ptr};

use windows_sys::Win32::System::Console::ENABLE_ECHO_INPUT;

use crate::console::ModeGuard;

/// Environment variable read by `-e`.
pub const ENV_VAR: &str = "SSHPASS";

/// Bytes that are overwritten with zeros before their memory is released.
///
/// This is best effort: copies made before the bytes reach a `Secret` (the
/// process command line, the environment block, std's stdin buffer) are out of
/// its reach.
#[derive(Default)]
pub struct Secret(Vec<u8>);

impl Secret {
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    fn push(&mut self, byte: u8) {
        if self.0.len() == self.0.capacity() {
            // Grow by hand: a `Vec` reallocation would free the old buffer unwiped.
            let mut grown = Vec::with_capacity((self.0.capacity() * 2).max(64));
            grown.extend_from_slice(&self.0);
            drop(Self(mem::replace(&mut self.0, grown)));
        }
        self.0.push(byte);
    }
}

impl From<String> for Secret {
    fn from(text: String) -> Self {
        Self(text.into_bytes())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: `byte` is a valid, exclusive reference into the buffer.
            unsafe { ptr::write_volatile(byte, 0) };
        }
        compiler_fence(Ordering::SeqCst);
    }
}

#[derive(Debug)]
pub enum PasswordSource {
    /// First line of standard input.
    Stdin,
    /// First line of a file (`-f`).
    File(PathBuf),
    /// First line read from an inherited handle (`-d`).
    Handle(NonZeroUsize),
    /// The `SSHPASS` environment variable (`-e`).
    Env,
    /// Given on the command line (`-p`).
    Argument(Secret),
}

impl PasswordSource {
    /// Obtains the password. This happens before the command starts, so a
    /// missing file or variable is reported without ever connecting.
    ///
    /// # Errors
    ///
    /// Fails if the source cannot be read or `SSHPASS` is unset or not Unicode.
    pub fn resolve(self) -> Result<Secret, PasswordError> {
        match self {
            Self::Stdin => read_stdin_line().map_err(PasswordError::Stdin),
            Self::File(path) => File::open(&path)
                .and_then(read_first_line)
                .map_err(|error| PasswordError::File { path, error }),
            Self::Handle(handle) => {
                read_handle_line(handle).map_err(|error| PasswordError::Handle { handle, error })
            }
            Self::Env => match env::var(ENV_VAR) {
                Ok(password) => Ok(Secret::from(password)),
                Err(VarError::NotPresent) => Err(PasswordError::EnvNotSet),
                Err(VarError::NotUnicode(_)) => Err(PasswordError::EnvNotUnicode),
            },
            Self::Argument(secret) => Ok(secret),
        }
    }
}

/// Reads up to the first `\r` or `\n`, or to the end of input.
///
/// # Errors
///
/// Propagates read errors from `reader`.
#[expect(
    clippy::unbuffered_bytes,
    reason = "a read buffer would hold password bytes that are freed without being wiped"
)]
pub fn read_first_line(reader: impl Read) -> io::Result<Secret> {
    let mut line = Secret::default();
    for byte in reader.bytes() {
        match byte? {
            b'\r' | b'\n' => break,
            byte => line.push(byte),
        }
    }
    Ok(line)
}

fn read_stdin_line() -> io::Result<Secret> {
    let stdin = io::stdin();
    let _echo_off = ModeGuard::update(stdin.as_handle(), |mode| mode & !ENABLE_ECHO_INPUT)?;
    read_first_line(stdin.lock())
}

fn read_handle_line(handle: NonZeroUsize) -> io::Result<Secret> {
    // SAFETY: `-d` names a handle the parent process opened for us. The borrow
    // only lives for the duplication below, and a value that is not an open
    // handle makes `DuplicateHandle` fail instead of touching any memory.
    let borrowed = unsafe { BorrowedHandle::borrow_raw(ptr::without_provenance_mut(handle.get())) };
    read_first_line(File::from(borrowed.try_clone_to_owned()?))
}

#[derive(Debug)]
pub enum PasswordError {
    EnvNotSet,
    EnvNotUnicode,
    Stdin(io::Error),
    File {
        path: PathBuf,
        error: io::Error,
    },
    Handle {
        handle: NonZeroUsize,
        error: io::Error,
    },
}

impl fmt::Display for PasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvNotSet => write!(
                f,
                "-e option given but {ENV_VAR} environment variable not set"
            ),
            Self::EnvNotUnicode => write!(f, "{ENV_VAR} environment variable is not valid Unicode"),
            Self::Stdin(_) => f.write_str("failed to read password from standard input"),
            Self::File { path, .. } => {
                write!(f, "failed to read password file \"{}\"", path.display())
            }
            Self::Handle { handle, .. } => {
                write!(f, "failed to read password from handle {handle}")
            }
        }
    }
}

impl std::error::Error for PasswordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EnvNotSet | Self::EnvNotUnicode => None,
            Self::Stdin(error) | Self::File { error, .. } | Self::Handle { error, .. } => {
                Some(error)
            }
        }
    }
}
