//! Running the command inside a pseudo console while threads relay output,
//! keystrokes and window size between it and the user's console.
//!
//! Thread layout:
//! - output pump: reads the pseudo console, drives the [`Detector`], writes
//!   stdout and types the password;
//! - waiter: blocks until the command exits;
//! - window tracker: polls the console size (only when stdout is a console);
//! - input forwarder: relays keystrokes (only when stdin is a console).
//!
//! The first three are scoped threads borrowing the session's state. The
//! forwarder is detached because it blocks in a console read that cannot be
//! interrupted; it ends with the process.

use std::convert::Infallible;
use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::os::windows::io::AsHandle;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::System::Console::{
    ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING,
};

use crate::cmdline::CommandLine;
use crate::conpty::{Child, PseudoConsole};
use crate::console::{self, ModeGuard, Size};
use crate::error::{Error, Status};
use crate::host_modes::HostModeFilter;
use crate::password::Secret;
use crate::prompt::{Detector, Failure, Reaction};
use crate::utf16::Utf16Decoder;

const WINDOW_POLL_INTERVAL: Duration = Duration::from_millis(100);
const OUTPUT_CHUNK_SIZE: usize = 8192;
/// What a terminal sends for the Enter key. The pseudo console turns a bare
/// `\n` into Ctrl+Enter, which console readers do not treat as end of line.
const ENTER: &[u8] = b"\r";

/// How a session that ran to completion ended.
#[derive(Debug)]
pub(crate) enum Outcome {
    Exited(u32),
    Rejected(Failure),
}

/// Reports from worker threads to the supervising thread.
enum Signal {
    Exited(io::Result<u32>),
    Rejected(Failure),
    Broken(io::Error),
}

pub(crate) fn run(
    mut command_line: CommandLine,
    password: &Secret,
    detector: Detector,
    verbose: bool,
) -> Result<Outcome, Error> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let _vt_output = ModeGuard::update(stdout.as_handle(), |mode| {
        mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING
    })
    .map_err(Error::io("enable virtual terminal output"))?;
    let window = console::window_size(stdout.as_handle());

    let (input_reader, input_writer) = io::pipe().map_err(Error::io("create input pipe"))?;
    let (output_reader, output_writer) = io::pipe().map_err(Error::io("create output pipe"))?;
    let pty = PseudoConsole::new(
        window.unwrap_or(Size::FALLBACK),
        input_reader,
        output_writer,
    )
    .map_err(Error::io("create pseudo console"))?;
    let child = pty
        .spawn(&mut command_line)
        .map_err(Error::io("start command"))?;

    let input = Arc::new(input_writer);
    let raw_input = ModeGuard::update(stdin.as_handle(), |mode| {
        mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT)
            | ENABLE_VIRTUAL_TERMINAL_INPUT
    })
    .map_err(Error::io("switch console input to raw mode"))?;
    if raw_input.is_some() {
        forward_input(Arc::clone(&input));
    }

    let (signals, inbox) = mpsc::channel();
    thread::scope(|scope| {
        let pump = OutputPump {
            input: &input,
            password,
            detector,
            host_modes: HostModeFilter::default(),
            shown: Vec::with_capacity(OUTPUT_CHUNK_SIZE),
            verbose,
            signals: signals.clone(),
        };
        scope.spawn(move || pump.run(output_reader));
        supervise(pty, &child, window, signals, &inbox)
    })
}

/// Waits for the first decisive signal, then shuts the pseudo console down.
///
/// The console is closed only after the window tracker borrowing it has been
/// joined; the pump then drains the final output and sees end-of-file.
fn supervise(
    pty: PseudoConsole,
    child: &Child,
    window: Option<Size>,
    signals: Sender<Signal>,
    inbox: &Receiver<Signal>,
) -> Result<Outcome, Error> {
    let (stop, stopped) = mpsc::channel::<Infallible>();
    let outcome = thread::scope(|scope| {
        if let Some(size) = window {
            let pty = &pty;
            scope.spawn(move || track_window_size(pty, size, &stopped));
        }
        scope.spawn(move || {
            // The supervisor may already have decided; a late report is moot.
            let _ = signals.send(Signal::Exited(child.wait()));
        });

        let signal = inbox.recv().expect("the waiter thread always reports");
        let outcome = match signal {
            Signal::Exited(code) => code
                .map(Outcome::Exited)
                .map_err(Error::io("wait for command")),
            Signal::Rejected(failure) => child
                .terminate(Status::from(failure).into())
                .map(|()| Outcome::Rejected(failure))
                .map_err(Error::io("stop command")),
            Signal::Broken(error) => {
                // Already failing; a second error from stopping the command adds nothing.
                let _ = child.terminate(Status::RuntimeError.into());
                Err(Error::io("relay command output")(error))
            }
        };
        drop(stop);
        outcome
    });
    drop(pty);
    outcome
}

fn track_window_size(pty: &PseudoConsole, mut current: Size, stopped: &Receiver<Infallible>) {
    let stdout = io::stdout();
    while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(WINDOW_POLL_INTERVAL) {
        if let Some(size) = console::window_size(stdout.as_handle())
            && size != current
        {
            if let Err(error) = pty.resize(size) {
                eprintln!("sshpass: failed to resize pseudo console: {error}");
                return;
            }
            current = size;
        }
    }
}

struct OutputPump<'a> {
    input: &'a PipeWriter,
    password: &'a Secret,
    detector: Detector,
    host_modes: HostModeFilter,
    /// Reused buffer for the filtered output of one chunk.
    shown: Vec<u8>,
    verbose: bool,
    signals: Sender<Signal>,
}

impl OutputPump<'_> {
    fn run(mut self, mut output: PipeReader) {
        let mut chunk = [0; OUTPUT_CHUNK_SIZE];
        let mut relaying = true;
        loop {
            let len = match output.read(&mut chunk) {
                Ok(0) => {
                    if relaying {
                        self.shown.clear();
                        self.host_modes.finish(&mut self.shown);
                        if let Err(error) = write_stdout(&self.shown) {
                            self.report(Signal::Broken(error));
                        }
                    }
                    return;
                }
                Ok(len) => len,
                Err(error) => {
                    self.report(Signal::Broken(error));
                    return;
                }
            };
            // After a failure keep reading anyway: closing the pseudo console
            // waits for its output to be drained.
            if relaying && let Err(error) = self.react(&chunk[..len]) {
                relaying = false;
                self.report(Signal::Broken(error));
            }
        }
    }

    fn react(&mut self, chunk: &[u8]) -> io::Result<()> {
        let mut input = self.input;
        match self.detector.feed(chunk) {
            Reaction::Hide(bytes) => self.log(format_args!("read: {}", bytes.escape_ascii())),
            Reaction::Forward(bytes) => {
                self.shown.clear();
                self.host_modes.filter(bytes, &mut self.shown);
                write_stdout(&self.shown)?;
            }
            Reaction::SendPassword => {
                self.log(format_args!("detected prompt. Sending password."));
                input.write_all(self.password.expose())?;
                input.write_all(ENTER)?;
            }
            Reaction::ConfirmHostKey => {
                self.log(format_args!(
                    "detected host authentication prompt. Confirming."
                ));
                input.write_all(b"yes")?;
                input.write_all(ENTER)?;
            }
            Reaction::Fail(failure) => self.report(Signal::Rejected(failure)),
        }
        Ok(())
    }

    fn report(&self, signal: Signal) {
        // The supervisor may already have decided; a late report is moot.
        let _ = self.signals.send(signal);
    }

    fn log(&self, message: std::fmt::Arguments<'_>) {
        if self.verbose {
            eprintln!("SSHPASS: {message}");
        }
    }
}

fn write_stdout(bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(bytes)?;
    stdout.flush()
}

fn forward_input(input: Arc<PipeWriter>) {
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut units = [0; 256];
        let mut decoder = Utf16Decoder::default();
        let mut text = String::new();
        // A failed read or write means the console or the pseudo console is
        // gone; either way nothing is left to relay.
        while let Ok(len @ 1..) = console::read_utf16(stdin.as_handle(), &mut units) {
            text.clear();
            decoder.decode(&units[..len], &mut text);
            if (&*input).write_all(text.as_bytes()).is_err() {
                break;
            }
        }
    });
}
