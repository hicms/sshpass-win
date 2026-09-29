//! Recognizing ssh's prompts in pseudo-console output.
//!
//! [`Detector`] performs no I/O: it is fed output chunks and answers with a
//! [`Reaction`], which keeps the protocol logic testable without a console.

use std::fmt;

/// Printed by OpenSSH before it waits for a yes/no answer about an unknown host key.
const HOST_KEY_PROMPT: &[u8] = b"Are you sure you want to continue connecting";
/// Printed by OpenSSH when the key stored for the host's IP address does not match.
const HOST_KEY_CHANGED: &[u8] = b"differs from the key for the IP address";
/// Complete lines of text that may precede a repeated prompt, such as
/// "Permission denied, please try again." Any more and the login succeeded;
/// a prompt later on belongs to the remote session.
const RETRY_MESSAGE_LINES: usize = 1;

/// Why a session was stopped before the command could run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    IncorrectPassword,
    HostKeyUnknown,
    HostKeyChanged,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IncorrectPassword => "incorrect password",
            Self::HostKeyUnknown => "host public key is unknown (use -k to accept it)",
            Self::HostKeyChanged => "host public key for the IP address has changed",
        })
    }
}

/// What to do with one chunk of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction<'a> {
    /// Keep the chunk from the user.
    Hide(&'a [u8]),
    /// Show the chunk to the user.
    Forward(&'a [u8]),
    /// The password prompt appeared: type the password.
    SendPassword,
    /// The host key question appeared and `-k` was given: answer "yes".
    ConfirmHostKey,
    /// Stop the command.
    Fail(Failure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before the password prompt. Output stays hidden: it belongs to the
    /// terminal conversation with ssh, not to the command's output.
    AwaitingPrompt,
    /// Password sent. Output is shown, but the prompt reappearing within the
    /// first [`RETRY_MESSAGE_LINES`] lines of text means it was rejected.
    ///
    /// While `prompt_line_open`, the prompt's own line has not ended yet. ssh
    /// ends it by echoing Enter, which is part of the terminal conversation
    /// and not of the command's output, so it is hidden here too.
    Verifying { prompt_line_open: bool },
    /// Logged in; the detector no longer interferes.
    Passthrough,
    /// A failure was reported; the rest is hidden while it drains.
    Finished,
}

#[derive(Debug, Clone)]
pub struct Detector {
    phase: Phase,
    auto_confirm: bool,
    password_prompt: Matcher,
    host_key_prompt: Matcher,
    host_key_changed: Matcher,
    lines: TextLines,
}

impl Detector {
    /// `prompt` is the text that marks a password prompt (`-P`).
    #[must_use]
    pub fn new(prompt: &str, auto_confirm: bool) -> Self {
        Self {
            phase: Phase::AwaitingPrompt,
            auto_confirm,
            password_prompt: Matcher::new(prompt.as_bytes()),
            host_key_prompt: Matcher::new(HOST_KEY_PROMPT),
            host_key_changed: Matcher::new(HOST_KEY_CHANGED),
            lines: TextLines::default(),
        }
    }

    pub fn feed<'a>(&mut self, chunk: &'a [u8]) -> Reaction<'a> {
        match self.phase {
            Phase::AwaitingPrompt => self.await_prompt(chunk),
            Phase::Verifying { prompt_line_open } => self.verify(chunk, prompt_line_open),
            Phase::Passthrough => Reaction::Forward(chunk),
            Phase::Finished => Reaction::Hide(chunk),
        }
    }

    fn await_prompt<'a>(&mut self, chunk: &'a [u8]) -> Reaction<'a> {
        // Every scanner sees every chunk so each keeps its state across chunk boundaries.
        let key_changed = self.host_key_changed.feed(chunk);
        let key_prompt = self.host_key_prompt.feed(chunk);
        let password_prompt = self.password_prompt.feed(chunk);
        self.lines.feed(chunk);

        match (key_changed, key_prompt, password_prompt) {
            (true, _, _) => self.fail(Failure::HostKeyChanged),
            (false, true, _) if self.auto_confirm => Reaction::ConfirmHostKey,
            (false, true, _) => self.fail(Failure::HostKeyUnknown),
            (false, false, true) => {
                self.phase = Phase::Verifying {
                    prompt_line_open: true,
                };
                // The prompt's own line does not count once the Enter echo ends it.
                self.lines.restart_count();
                Reaction::SendPassword
            }
            (false, false, false) => Reaction::Hide(chunk),
        }
    }

    fn verify<'a>(&mut self, chunk: &'a [u8], prompt_line_open: bool) -> Reaction<'a> {
        if self.password_prompt.feed(chunk) {
            return self.fail(Failure::IncorrectPassword);
        }
        let blank_line_end = self.lines.feed(chunk);
        let prompt_line_end = match (prompt_line_open, blank_line_end) {
            (false, _) => 0,
            (true, Some(end)) => end,
            (true, None) if self.lines.has_text() => 0,
            (true, None) => return Reaction::Hide(chunk),
        };
        self.phase = if self.lines.completed > RETRY_MESSAGE_LINES {
            Phase::Passthrough
        } else {
            Phase::Verifying {
                prompt_line_open: false,
            }
        };
        match chunk.split_at(prompt_line_end) {
            (hidden, []) => Reaction::Hide(hidden),
            (_, shown) => Reaction::Forward(shown),
        }
    }

    fn fail<'a>(&mut self, failure: Failure) -> Reaction<'a> {
        self.phase = Phase::Finished;
        Reaction::Fail(failure)
    }
}

/// Streaming substring search (Knuth–Morris–Pratt), so a needle split across
/// chunks is still found without buffering any output.
#[derive(Debug, Clone)]
struct Matcher {
    needle: Box<[u8]>,
    /// `fallback[i]` is the length of the longest proper prefix of
    /// `needle[..=i]` that is also a suffix of it.
    fallback: Box<[usize]>,
    matched: usize,
}

impl Matcher {
    fn new(needle: &[u8]) -> Self {
        let mut fallback = vec![0; needle.len()];
        let mut len = 0;
        for i in 1..needle.len() {
            while len > 0 && needle[i] != needle[len] {
                len = fallback[len - 1];
            }
            if needle[i] == needle[len] {
                len += 1;
            }
            fallback[i] = len;
        }
        Self {
            needle: needle.into(),
            fallback: fallback.into(),
            matched: 0,
        }
    }

    /// Returns whether an occurrence of the needle ends inside `bytes`.
    fn feed(&mut self, bytes: &[u8]) -> bool {
        if self.needle.is_empty() {
            return true;
        }
        let mut found = false;
        for &byte in bytes {
            while self.matched > 0 && self.needle[self.matched] != byte {
                self.matched = self.fallback[self.matched - 1];
            }
            if self.needle[self.matched] == byte {
                self.matched += 1;
            }
            if self.matched == self.needle.len() {
                found = true;
                self.matched = self.fallback[self.matched - 1];
            }
        }
        found
    }
}

const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;
const DEL: u8 = 0x7F;

/// Counts completed lines that contain visible text, so blank lines and
/// terminal control traffic are not mistaken for output.
#[derive(Debug, Clone, Default)]
struct TextLines {
    escape: Escape,
    line_has_text: bool,
    completed: usize,
}

impl TextLines {
    /// Returns the offset just past the first line break in `bytes` if the
    /// line it ends had no text.
    fn feed(&mut self, bytes: &[u8]) -> Option<usize> {
        let mut first_break = None;
        for (offset, &byte) in bytes.iter().enumerate() {
            match self.escape.advance(byte) {
                Some(b'\n') => {
                    if first_break.is_none() {
                        first_break = Some((!self.line_has_text).then_some(offset + 1));
                    }
                    if self.line_has_text {
                        self.line_has_text = false;
                        self.completed += 1;
                    }
                }
                Some(byte) if byte > b' ' && byte != DEL => self.line_has_text = true,
                _ => {}
            }
        }
        first_break.flatten()
    }

    /// Whether any text appeared since the count was restarted.
    fn has_text(&self) -> bool {
        self.line_has_text || self.completed > 0
    }

    fn restart_count(&mut self) {
        self.line_has_text = false;
        self.completed = 0;
    }
}

/// VT escape-sequence state, carried across chunks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Escape {
    #[default]
    Ground,
    Start,
    /// Control Sequence (`ESC [`), ended by a final byte in `@`..=`~`.
    Csi,
    /// OSC, DCS, SOS, PM or APC string, ended by BEL or `ESC \`.
    String,
    StringEnd,
}

impl Escape {
    /// Consumes `byte`, handing it back if it is not part of an escape sequence.
    fn advance(&mut self, byte: u8) -> Option<u8> {
        let (next, plain) = match (*self, byte) {
            // 0x20..=0x2F are intermediate bytes, as in `ESC ( B`.
            (Self::Ground, ESC) | (Self::Start, 0x20..=0x2F) => (Self::Start, None),
            (Self::Ground, byte) => (Self::Ground, Some(byte)),
            (Self::Start, b'[') => (Self::Csi, None),
            (Self::Start, b']' | b'P' | b'X' | b'^' | b'_') => (Self::String, None),
            (Self::Csi, 0x40..=0x7E) | (Self::Start | Self::StringEnd, _) | (Self::String, BEL) => {
                (Self::Ground, None)
            }
            (Self::Csi, _) => (Self::Csi, None),
            (Self::String, ESC) => (Self::StringEnd, None),
            (Self::String, _) => (Self::String, None),
        };
        *self = next;
        plain
    }
}
