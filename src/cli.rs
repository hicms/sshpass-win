//! Command-line parsing with the semantics of POSIX `getopt("+f:d:p:P:hvek")`:
//! short options may be clustered (`-vk`), values may be attached
//! (`-pSECRET`) or separate (`-p SECRET`), and option parsing stops at `--`
//! or at the first operand, which starts the command.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::num::NonZeroUsize;

use crate::password::{ENV_VAR, PasswordSource, Secret};

/// Text sshpass looks for when `-P` is not given.
pub const DEFAULT_PROMPT: &str = "password:";

/// The help screen, printed by `-h` and after usage errors.
#[derive(Debug, Clone, Copy)]
pub struct Usage;

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\
Usage: sshpass [-f|-d|-p|-e] [-hvk] [-P prompt] command arguments
   -f filename   Take password to use from file
   -d number     Use number as file handle for getting password
   -p password   Provide password as argument (security unwise)
   -e            Password is passed as env-var \"{ENV_VAR}\"
   With no parameters - password will be taken from stdin

   -P prompt     Which string should sshpass search for to detect a password prompt
                 (default \"{DEFAULT_PROMPT}\")
   -v            Be verbose about what you're doing
   -k, --auto-confirm
                 Auto confirm 'Are you sure...' prompt for host keys
   -h, --help    Show help (this screen)
At most one of -f, -d, -p or -e should be used
"
        )
    }
}

#[derive(Debug)]
pub enum Invocation {
    Help,
    Run(Options),
}

#[derive(Debug)]
pub struct Options {
    pub password: PasswordSource,
    pub prompt: String,
    pub verbose: bool,
    pub auto_confirm: bool,
    pub program: OsString,
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    UnknownOption(String),
    MissingValue(char),
    NotUnicode(char),
    InvalidHandle(String),
    EmptyPrompt,
    ConflictingSource,
    MissingCommand,
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOption(option) => write!(f, "unknown option '{option}'"),
            Self::MissingValue(flag) => write!(f, "option requires an argument -- '{flag}'"),
            Self::NotUnicode(flag) => write!(f, "the value of -{flag} is not valid Unicode"),
            Self::InvalidHandle(value) => write!(f, "invalid handle number '{value}' for -d"),
            Self::EmptyPrompt => f.write_str("the prompt given with -P must not be empty"),
            Self::ConflictingSource => f.write_str("Conflicting password source"),
            Self::MissingCommand => f.write_str("missing command"),
        }
    }
}

impl std::error::Error for UsageError {}

/// Parses the arguments that follow the program name.
///
/// # Errors
///
/// Returns a [`UsageError`] for unknown options, missing values, several
/// password sources, or a missing command.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Invocation, UsageError> {
    let mut args = args.into_iter();
    let mut builder = Builder::default();

    let program = loop {
        let Some(arg) = args.next() else {
            return Err(UsageError::MissingCommand);
        };
        let bytes = arg.as_encoded_bytes();
        // Like getopt, a lone "-" is an operand rather than an option.
        if bytes.len() < 2 || bytes[0] != b'-' {
            break arg;
        }
        let Some(text) = arg.to_str() else {
            return Err(UsageError::UnknownOption(
                arg.to_string_lossy().into_owned(),
            ));
        };
        if text == "--" {
            break args.next().ok_or(UsageError::MissingCommand)?;
        }
        if let Some(name) = text.strip_prefix("--") {
            match name {
                "help" => return Ok(Invocation::Help),
                "auto-confirm" => builder.auto_confirm = true,
                _ => return Err(UsageError::UnknownOption(text.to_owned())),
            }
            continue;
        }

        let cluster = &text[1..];
        for (at, letter) in cluster.char_indices() {
            match Short::from_letter(letter) {
                Some(Short::Help) => return Ok(Invocation::Help),
                Some(Short::Verbose) => builder.verbose = true,
                Some(Short::AutoConfirm) => builder.auto_confirm = true,
                Some(Short::Env) => builder.set_source(PasswordSource::Env)?,
                Some(Short::Valued(option)) => {
                    let attached = &cluster[at + letter.len_utf8()..];
                    let value = if attached.is_empty() {
                        args.next().ok_or(UsageError::MissingValue(letter))?
                    } else {
                        attached.into()
                    };
                    builder.apply(option, value)?;
                    break;
                }
                None => return Err(UsageError::UnknownOption(format!("-{letter}"))),
            }
        }
    };

    Ok(Invocation::Run(Options {
        password: builder.password.unwrap_or(PasswordSource::Stdin),
        prompt: builder.prompt.unwrap_or_else(|| DEFAULT_PROMPT.to_owned()),
        verbose: builder.verbose,
        auto_confirm: builder.auto_confirm,
        program,
        args: args.collect(),
    }))
}

#[derive(Debug, Clone, Copy)]
enum Short {
    Help,
    Verbose,
    AutoConfirm,
    Env,
    Valued(Valued),
}

#[derive(Debug, Clone, Copy)]
enum Valued {
    File,
    Handle,
    Password,
    Prompt,
}

impl Valued {
    const fn letter(self) -> char {
        match self {
            Self::File => 'f',
            Self::Handle => 'd',
            Self::Password => 'p',
            Self::Prompt => 'P',
        }
    }
}

impl Short {
    fn from_letter(letter: char) -> Option<Self> {
        Some(match letter {
            'h' => Self::Help,
            'v' => Self::Verbose,
            'k' => Self::AutoConfirm,
            'e' => Self::Env,
            'f' => Self::Valued(Valued::File),
            'd' => Self::Valued(Valued::Handle),
            'p' => Self::Valued(Valued::Password),
            'P' => Self::Valued(Valued::Prompt),
            _ => return None,
        })
    }
}

#[derive(Debug, Default)]
struct Builder {
    password: Option<PasswordSource>,
    prompt: Option<String>,
    verbose: bool,
    auto_confirm: bool,
}

impl Builder {
    fn set_source(&mut self, source: PasswordSource) -> Result<(), UsageError> {
        match self.password.replace(source) {
            None => Ok(()),
            Some(_) => Err(UsageError::ConflictingSource),
        }
    }

    fn apply(&mut self, option: Valued, value: OsString) -> Result<(), UsageError> {
        match option {
            Valued::File => self.set_source(PasswordSource::File(value.into())),
            Valued::Handle => self.set_source(PasswordSource::Handle(parse_handle(&value)?)),
            Valued::Password => {
                let password = into_unicode(option, value)?;
                self.set_source(PasswordSource::Argument(Secret::from(password)))
            }
            Valued::Prompt => {
                let prompt = into_unicode(option, value)?;
                if prompt.is_empty() {
                    return Err(UsageError::EmptyPrompt);
                }
                self.prompt = Some(prompt);
                Ok(())
            }
        }
    }
}

fn into_unicode(option: Valued, value: OsString) -> Result<String, UsageError> {
    value
        .into_string()
        .map_err(|_| UsageError::NotUnicode(option.letter()))
}

/// Accepts decimal or `0x`-prefixed hexadecimal; handle 0 (`NULL`) is never valid.
fn parse_handle(value: &OsStr) -> Result<NonZeroUsize, UsageError> {
    value
        .to_str()
        .and_then(
            |text| match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                Some(hex) => usize::from_str_radix(hex, 16).ok(),
                None => text.parse().ok(),
            },
        )
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| UsageError::InvalidHandle(value.to_string_lossy().into_owned()))
}
