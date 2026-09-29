//! Exit statuses and the error type that maps onto them.

use std::{fmt, io};

use crate::cli::UsageError;
use crate::password::PasswordError;
use crate::prompt::Failure;

/// Exit statuses for failures of sshpass itself.
///
/// Once the command has started and logged in, sshpass exits with the
/// command's own exit code instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    InvalidArguments = 1,
    ConflictingArguments = 2,
    RuntimeError = 3,
    IncorrectPassword = 5,
    HostKeyUnknown = 6,
    HostKeyChanged = 7,
}

impl From<Status> for u32 {
    fn from(status: Status) -> Self {
        Self::from(status as u8)
    }
}

impl From<Failure> for Status {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::IncorrectPassword => Self::IncorrectPassword,
            Failure::HostKeyUnknown => Self::HostKeyUnknown,
            Failure::HostKeyChanged => Self::HostKeyChanged,
        }
    }
}

/// Everything that makes sshpass exit with one of its own [`Status`] codes.
#[derive(Debug)]
pub enum Error {
    Usage(UsageError),
    Password(PasswordError),
    Io {
        context: &'static str,
        source: io::Error,
    },
    Rejected(Failure),
}

impl Error {
    /// Builds a `map_err` adapter that tags an I/O error with what was being attempted.
    pub(crate) fn io(context: &'static str) -> impl FnOnce(io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    #[must_use]
    pub fn status(&self) -> Status {
        match self {
            Self::Usage(UsageError::ConflictingSource) => Status::ConflictingArguments,
            Self::Usage(_)
            | Self::Password(PasswordError::EnvNotSet | PasswordError::EnvNotUnicode) => {
                Status::InvalidArguments
            }
            Self::Password(_) | Self::Io { .. } => Status::RuntimeError,
            Self::Rejected(failure) => Status::from(*failure),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => error.fmt(f),
            Self::Password(error) => error.fmt(f),
            Self::Io { context, .. } => write!(f, "failed to {context}"),
            Self::Rejected(failure) => failure.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage(error) => error.source(),
            Self::Password(error) => error.source(),
            Self::Io { source, .. } => Some(source),
            Self::Rejected(_) => None,
        }
    }
}

impl From<UsageError> for Error {
    fn from(error: UsageError) -> Self {
        Self::Usage(error)
    }
}

impl From<PasswordError> for Error {
    fn from(error: PasswordError) -> Self {
        Self::Password(error)
    }
}
