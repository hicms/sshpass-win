//! Building the command line handed to `CreateProcessW`.

use std::ffi::OsStr;
use std::fmt::{self, Write as _};
use std::iter;
use std::os::windows::ffi::OsStrExt;

const SPACE: u16 = 0x20;
const TAB: u16 = 0x09;
const NEWLINE: u16 = 0x0A;
const VERTICAL_TAB: u16 = 0x0B;
const QUOTE: u16 = 0x22;
const BACKSLASH: u16 = 0x5C;

/// A NUL-terminated UTF-16 command line.
///
/// Arguments are quoted by the rules of `CommandLineToArgvW` and the MSVC C
/// runtime, so the child splits it back into exactly the arguments given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine(Vec<u16>);

impl CommandLine {
    pub fn new<A: AsRef<OsStr>>(
        program: impl AsRef<OsStr>,
        args: impl IntoIterator<Item = A>,
    ) -> Self {
        let mut line = Vec::new();
        append_quoted(&mut line, program.as_ref());
        for arg in args {
            line.push(SPACE);
            append_quoted(&mut line, arg.as_ref());
        }
        line.push(0);
        Self(line)
    }

    /// `CreateProcessW` may write to the buffer, hence the mutable pointer.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut u16 {
        self.0.as_mut_ptr()
    }

    fn units(&self) -> &[u16] {
        &self.0[..self.0.len() - 1]
    }
}

impl fmt::Display for CommandLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        char::decode_utf16(self.units().iter().copied())
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .try_for_each(|c| f.write_char(c))
    }
}

fn append_quoted(line: &mut Vec<u16>, arg: &OsStr) {
    let needs_quotes = arg.is_empty()
        || arg
            .encode_wide()
            .any(|unit| matches!(unit, SPACE | TAB | NEWLINE | VERTICAL_TAB | QUOTE));
    if !needs_quotes {
        line.extend(arg.encode_wide());
        return;
    }

    // Backslashes are literal unless they precede a quote, where each one
    // must be doubled; the closing quote counts as well.
    line.push(QUOTE);
    let mut backslashes = 0;
    for unit in arg.encode_wide() {
        match unit {
            BACKSLASH => backslashes += 1,
            QUOTE => {
                line.extend(iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
                line.push(QUOTE);
                backslashes = 0;
            }
            _ => {
                line.extend(iter::repeat_n(BACKSLASH, backslashes));
                line.push(unit);
                backslashes = 0;
            }
        }
    }
    line.extend(iter::repeat_n(BACKSLASH, backslashes * 2));
    line.push(QUOTE);
}
