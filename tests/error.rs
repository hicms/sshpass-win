use std::error::Error as _;
use std::io;
use std::path::PathBuf;

use sshpass::cli::UsageError;
use sshpass::password::PasswordError;
use sshpass::prompt::Failure;
use sshpass::{Error, Status};

#[test]
fn statuses_have_stable_exit_codes() {
    let codes = [
        (Status::InvalidArguments, 1),
        (Status::ConflictingArguments, 2),
        (Status::RuntimeError, 3),
        (Status::IncorrectPassword, 5),
        (Status::HostKeyUnknown, 6),
        (Status::HostKeyChanged, 7),
    ];
    for (status, code) in codes {
        assert_eq!(u32::from(status), code, "{status:?}");
    }
}

#[test]
fn errors_map_to_statuses() {
    let cases = [
        (
            Error::Usage(UsageError::MissingCommand),
            Status::InvalidArguments,
        ),
        (
            Error::Usage(UsageError::ConflictingSource),
            Status::ConflictingArguments,
        ),
        (
            Error::Password(PasswordError::EnvNotSet),
            Status::InvalidArguments,
        ),
        (
            Error::Password(PasswordError::File {
                path: PathBuf::from("pw"),
                error: io::Error::other("gone"),
            }),
            Status::RuntimeError,
        ),
        (
            Error::Io {
                context: "start command",
                source: io::Error::other("gone"),
            },
            Status::RuntimeError,
        ),
        (
            Error::Rejected(Failure::IncorrectPassword),
            Status::IncorrectPassword,
        ),
        (
            Error::Rejected(Failure::HostKeyUnknown),
            Status::HostKeyUnknown,
        ),
        (
            Error::Rejected(Failure::HostKeyChanged),
            Status::HostKeyChanged,
        ),
    ];
    for (error, status) in cases {
        assert_eq!(error.status(), status, "{error:?}");
    }
}

#[test]
fn io_errors_keep_their_cause() {
    let error = Error::Io {
        context: "start command",
        source: io::Error::other("not found"),
    };
    assert_eq!(error.to_string(), "failed to start command");
    assert_eq!(
        error.source().map(ToString::to_string).as_deref(),
        Some("not found")
    );
}
