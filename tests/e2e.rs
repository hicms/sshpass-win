//! End-to-end tests: the real `sshpass` binary runs this test executable as a
//! fake `ssh` inside a real pseudo console.
//!
//! The target uses `harness = false` so one executable can play both roles:
//! with `SSHPASS_E2E_FAKE` set it behaves as the fake ssh, otherwise it runs
//! the test cases.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
use windows_sys::Win32::System::Console::{
    ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, GetConsoleMode, SetConsoleMode,
};

const SSHPASS: &str = env!("CARGO_BIN_EXE_sshpass");
const MODE_VAR: &str = "SSHPASS_E2E_FAKE";
const PROMPT_VAR: &str = "SSHPASS_E2E_PROMPT";
const DEFAULT_FAKE_PROMPT: &str = "user@host's password: ";
/// Non-ASCII on purpose: the password travels as UTF-8 through the pseudo console.
const PASSWORD: &str = "s3cr\u{e9}t-p\u{e4}ss";
const LOGIN_EXIT: u8 = 42;
const NO_PROMPT_EXIT: u8 = 17;
const TIMEOUT: Duration = Duration::from_secs(30);

type Outcome = Result<(), String>;
type Case = (&'static str, fn() -> Outcome);

macro_rules! ensure {
    ($condition:expr, $($message:tt)+) => {
        if !$condition {
            return Err(format!($($message)+));
        }
    };
}

fn main() -> ExitCode {
    if let Ok(mode) = env::var(MODE_VAR) {
        return fake_ssh(&mode);
    }

    let cases: &[Case] = &[
        ("password_argument_logs_in", password_argument_logs_in),
        ("arguments_arrive_unchanged", arguments_arrive_unchanged),
        ("wrong_password_exits_5", wrong_password_exits_5),
        ("password_file_first_line", password_file_first_line),
        (
            "missing_password_file_exits_3",
            missing_password_file_exits_3,
        ),
        ("env_password", env_password),
        ("env_unset_exits_1", env_unset_exits_1),
        ("stdin_password", stdin_password),
        ("handle_password", handle_password),
        ("custom_prompt", custom_prompt),
        ("host_key_unknown_exits_6", host_key_unknown_exits_6),
        (
            "auto_confirm_accepts_host_key",
            auto_confirm_accepts_host_key,
        ),
        ("host_key_changed_exits_7", host_key_changed_exits_7),
        ("exit_code_without_prompt", exit_code_without_prompt),
        ("missing_program_exits_3", missing_program_exits_3),
        ("usage_errors", usage_errors),
        ("help_exits_0", help_exits_0),
        ("verbose_reports_progress", verbose_reports_progress),
    ];

    let results: Vec<_> = thread::scope(|scope| {
        let handles: Vec<_> = cases
            .iter()
            .map(|&(name, case)| (name, scope.spawn(case)))
            .collect();
        handles
            .into_iter()
            .map(|(name, handle)| {
                (
                    name,
                    handle.join().unwrap_or_else(|_| Err("panicked".into())),
                )
            })
            .collect()
    });

    let mut failed = 0;
    for (name, result) in &results {
        match result {
            Ok(()) => println!("test {name} ... ok"),
            Err(why) => {
                failed += 1;
                println!("test {name} ... FAILED\n{why}\n");
            }
        }
    }
    let verdict = if failed == 0 { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {verdict}. {} passed; {failed} failed",
        results.len() - failed
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------------------
// Cases

fn password_argument_logs_in() -> Outcome {
    let run = Sshpass::new(&["-p", PASSWORD])
        .fake("login", &["a"])
        .run()?;
    run.expect_code(LOGIN_EXIT)?;
    ensure!(
        run.stdout.contains("LOGIN-OK"),
        "login banner missing\n{run}"
    );
    ensure!(
        !run.stdout.contains("password:"),
        "prompt should be hidden\n{run}"
    );
    ensure!(
        !run.stdout.starts_with("\r\n"),
        "the Enter echo should be hidden\n{run}"
    );
    ensure!(
        !run.stdout.contains("\x1b[?9001") && !run.stdout.contains("\x1b[?1004"),
        "pseudo console mode switches should be removed\n{run}"
    );
    Ok(())
}

fn arguments_arrive_unchanged() -> Outcome {
    let args = ["plain", "two words", "a\"b", "c\\", "", "d\\\"e"];
    let run = Sshpass::new(&["-p", PASSWORD]).fake("login", &args).run()?;
    run.expect_code(LOGIN_EXIT)?;
    let expected = format!("LOGIN-OK {args:?}");
    ensure!(run.stdout.contains(&expected), "expected {expected}\n{run}");
    Ok(())
}

fn wrong_password_exits_5() -> Outcome {
    let run = Sshpass::new(&["-p", "wrong"]).fake("login", &[]).run()?;
    run.expect_code(5)?;
    ensure!(
        run.stderr.contains("incorrect password"),
        "missing message\n{run}"
    );
    Ok(())
}

fn password_file_first_line() -> Outcome {
    let path = env::temp_dir().join(format!("sshpass-e2e-{}.txt", std::process::id()));
    fs::write(&path, format!("{PASSWORD}\r\nsecond line\r\n")).map_err(|e| e.to_string())?;
    let result = Sshpass::new(&["-f"])
        .arg(path.as_os_str())
        .fake("login", &[])
        .run();
    let _ = fs::remove_file(&path);
    result?.expect_code(LOGIN_EXIT)
}

fn missing_password_file_exits_3() -> Outcome {
    let run = Sshpass::new(&["-f", r"C:\sshpass-e2e\does-not-exist.txt"])
        .fake("login", &[])
        .run()?;
    run.expect_code(3)?;
    ensure!(
        run.stderr.contains("failed to read password file"),
        "missing message\n{run}"
    );
    Ok(())
}

fn env_password() -> Outcome {
    Sshpass::new(&["-e"])
        .env("SSHPASS", PASSWORD)
        .fake("login", &[])
        .run()?
        .expect_code(LOGIN_EXIT)
}

fn env_unset_exits_1() -> Outcome {
    let mut sshpass = Sshpass::new(&["-e"]).fake("login", &[]);
    sshpass.command.env_remove("SSHPASS");
    let run = sshpass.run()?;
    run.expect_code(1)?;
    ensure!(
        run.stderr.contains("SSHPASS environment variable not set"),
        "missing message\n{run}"
    );
    Ok(())
}

fn stdin_password() -> Outcome {
    Sshpass::new(&[])
        .fake("login", &[])
        .stdin(format!("{PASSWORD}\nleftover\n"))
        .run()?
        .expect_code(LOGIN_EXIT)
}

fn handle_password() -> Outcome {
    let (reader, mut writer) = io::pipe().map_err(|e| e.to_string())?;
    writer
        .write_all(format!("{PASSWORD}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    drop(writer);
    // SAFETY: `reader` is an open handle owned by this function.
    let inheritable = unsafe {
        SetHandleInformation(
            reader.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    };
    ensure!(inheritable != 0, "SetHandleInformation failed");
    let handle = (reader.as_raw_handle() as usize).to_string();
    let run = Sshpass::new(&["-d", &handle]).fake("login", &[]).run()?;
    drop(reader);
    run.expect_code(LOGIN_EXIT)
}

fn custom_prompt() -> Outcome {
    Sshpass::new(&["-P", "Passcode:", "-p", PASSWORD])
        .env(PROMPT_VAR, "Passcode: ")
        .fake("login", &[])
        .run()?
        .expect_code(LOGIN_EXIT)
}

fn host_key_unknown_exits_6() -> Outcome {
    Sshpass::new(&["-p", PASSWORD])
        .fake("hostkey", &[])
        .run()?
        .expect_code(6)
}

fn auto_confirm_accepts_host_key() -> Outcome {
    let run = Sshpass::new(&["-k", "-p", PASSWORD])
        .fake("hostkey", &[])
        .run()?;
    run.expect_code(LOGIN_EXIT)?;
    ensure!(
        run.stdout.contains("LOGIN-OK"),
        "login banner missing\n{run}"
    );
    Ok(())
}

fn host_key_changed_exits_7() -> Outcome {
    Sshpass::new(&["--auto-confirm", "-p", PASSWORD])
        .fake("ipchanged", &[])
        .run()?
        .expect_code(7)
}

fn exit_code_without_prompt() -> Outcome {
    Sshpass::new(&["-p", PASSWORD])
        .fake("nologin", &[])
        .run()?
        .expect_code(NO_PROMPT_EXIT)
}

fn missing_program_exits_3() -> Outcome {
    let run = Sshpass::new(&["-p", PASSWORD, r"C:\sshpass-e2e\missing.exe"]).run()?;
    run.expect_code(3)?;
    ensure!(
        run.stderr.contains("failed to start command"),
        "missing message\n{run}"
    );
    Ok(())
}

fn usage_errors() -> Outcome {
    let cases: [(&[&str], u8); 4] = [
        (&[], 1),
        (&["-x", "ssh"], 1),
        (&["-p"], 1),
        (&["-p", "a", "-e", "ssh"], 2),
    ];
    for (args, code) in cases {
        let run = Sshpass::new(args).run()?;
        run.expect_code(code)?;
        ensure!(
            run.stderr.contains("Usage: sshpass"),
            "usage missing for {args:?}\n{run}"
        );
    }
    Ok(())
}

fn help_exits_0() -> Outcome {
    let run = Sshpass::new(&["-h"]).run()?;
    run.expect_code(0)?;
    ensure!(
        run.stdout.starts_with("Usage: sshpass"),
        "help missing\n{run}"
    );
    Ok(())
}

fn verbose_reports_progress() -> Outcome {
    let run = Sshpass::new(&["-v", "-p", PASSWORD])
        .fake("login", &[])
        .run()?;
    run.expect_code(LOGIN_EXIT)?;
    for needle in [
        "SSHPASS: cmd: ",
        "SSHPASS: detected prompt. Sending password.",
    ] {
        ensure!(
            run.stderr.contains(needle),
            "stderr lacks {needle:?}\n{run}"
        );
    }
    ensure!(
        !run.stdout.contains("SSHPASS:"),
        "verbose output belongs on stderr\n{run}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Driving sshpass

struct Sshpass {
    command: Command,
    stdin: Option<String>,
}

impl Sshpass {
    fn new(args: &[&str]) -> Self {
        let mut command = Command::new(SSHPASS);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Self {
            command,
            stdin: None,
        }
    }

    fn arg(mut self, arg: &OsStr) -> Self {
        self.command.arg(arg);
        self
    }

    fn env(mut self, key: &str, value: &str) -> Self {
        self.command.env(key, value);
        self
    }

    fn stdin(mut self, input: String) -> Self {
        self.command.stdin(Stdio::piped());
        self.stdin = Some(input);
        self
    }

    /// Appends this executable, in fake-ssh `mode`, as the command to run.
    fn fake(mut self, mode: &str, args: &[&str]) -> Self {
        let me = env::current_exe().expect("current exe");
        self.command.env(MODE_VAR, mode).arg(me).args(args);
        self
    }

    fn run(mut self) -> Result<Run, String> {
        let mut child = self
            .command
            .spawn()
            .map_err(|e| format!("spawn sshpass: {e}"))?;
        if let (Some(input), Some(mut pipe)) = (self.stdin, child.stdin.take()) {
            pipe.write_all(input.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let stdout = collect(child.stdout.take());
        let stderr = collect(child.stderr.take());

        let deadline = Instant::now() + TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break Some(status);
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(20));
        };
        let run = Run {
            code: status.and_then(|status| status.code()),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        };
        if status.is_none() {
            return Err(format!("timed out\n{run}"));
        }
        Ok(run)
    }
}

fn collect(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    fn expect_code(&self, code: u8) -> Outcome {
        ensure!(
            self.code == Some(i32::from(code)),
            "expected exit code {code}\n{self}"
        );
        Ok(())
    }
}

impl std::fmt::Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "  exit code: {:?}\n  stdout: {:?}\n  stderr: {:?}",
            self.code, self.stdout, self.stderr
        )
    }
}

// ---------------------------------------------------------------------------
// The fake ssh

fn fake_ssh(mode: &str) -> ExitCode {
    raw_console_input();
    let prompt = env::var(PROMPT_VAR).unwrap_or_else(|_| DEFAULT_FAKE_PROMPT.to_owned());
    match mode {
        "login" => login(&prompt),
        "hostkey" => {
            say(
                "The authenticity of host 'host (10.0.0.1)' can't be established.\r\n\
                 Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            );
            if read_line() != "yes" {
                return ExitCode::from(99);
            }
            say("\r\n");
            login(&prompt)
        }
        "ipchanged" => {
            say(
                "Warning: the ECDSA host key for 'host' differs from the key for the IP address '10.0.0.1'\r\n",
            );
            say("Are you sure you want to continue connecting (yes/no)? ");
            read_line();
            ExitCode::from(98)
        }
        "nologin" => {
            say("no prompt here\r\n");
            ExitCode::from(NO_PROMPT_EXIT)
        }
        _ => ExitCode::from(97),
    }
}

/// Behaves like OpenSSH password authentication.
fn login(prompt: &str) -> ExitCode {
    say(prompt);
    loop {
        let answer = read_line();
        say("\r\n");
        if answer == PASSWORD {
            let args: Vec<String> = env::args().skip(1).collect();
            say(&format!("LOGIN-OK {args:?}\r\n"));
            return ExitCode::from(LOGIN_EXIT);
        }
        // The verdict follows the line break only after a server round trip.
        thread::sleep(Duration::from_millis(300));
        say(&format!("Permission denied, please try again.\r\n{prompt}"));
    }
}

fn say(text: &str) {
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

fn read_line() -> String {
    let mut line = Vec::new();
    for byte in io::stdin().lock().bytes() {
        match byte {
            Ok(b'\r' | b'\n') | Err(_) => break,
            Ok(byte) => line.push(byte),
        }
    }
    String::from_utf8_lossy(&line).into_owned()
}

/// Reads keystrokes one by one without echo, as ssh does for passwords.
fn raw_console_input() {
    let stdin = io::stdin().as_raw_handle();
    let mut mode = 0;
    // SAFETY: `stdin` is this process's console input handle and `mode` is a
    // valid out-pointer.
    unsafe {
        if GetConsoleMode(stdin, &raw mut mode) != 0 {
            SetConsoleMode(stdin, mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT));
        }
    }
}
