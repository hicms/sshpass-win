use std::ffi::OsString;
use std::path::Path;

use sshpass::cli::{self, DEFAULT_PROMPT, Invocation, Options, Usage, UsageError};
use sshpass::password::PasswordSource;

fn parse(args: &[&str]) -> Result<Invocation, UsageError> {
    cli::parse(args.iter().map(OsString::from))
}

fn options(args: &[&str]) -> Options {
    match parse(args) {
        Ok(Invocation::Run(options)) => options,
        other => panic!("expected options for {args:?}, got {other:?}"),
    }
}

fn error(args: &[&str]) -> UsageError {
    match parse(args) {
        Err(error) => error,
        other => panic!("expected an error for {args:?}, got {other:?}"),
    }
}

fn password_argument(options: &Options) -> &[u8] {
    match &options.password {
        PasswordSource::Argument(secret) => secret.expose(),
        other => panic!("expected -p, got {other:?}"),
    }
}

#[test]
fn defaults_without_options() {
    let options = options(&["ssh", "host"]);
    assert!(matches!(options.password, PasswordSource::Stdin));
    assert_eq!(options.prompt, DEFAULT_PROMPT);
    assert!(!options.verbose);
    assert!(!options.auto_confirm);
    assert_eq!(options.program, "ssh");
    assert_eq!(options.args, ["host"]);
}

#[test]
fn options_stop_at_the_first_operand() {
    let options = options(&["-p", "pw", "ssh", "-v", "-p", "host"]);
    assert!(!options.verbose);
    assert_eq!(options.program, "ssh");
    assert_eq!(options.args, ["-v", "-p", "host"]);
}

#[test]
fn double_dash_ends_options() {
    let options = options(&["-v", "--", "-odd-program", "x"]);
    assert!(options.verbose);
    assert_eq!(options.program, "-odd-program");
    assert_eq!(options.args, ["x"]);
}

#[test]
fn lone_dash_is_an_operand() {
    assert_eq!(options(&["-", "x"]).program, "-");
}

#[test]
fn flags_can_be_clustered() {
    let options = options(&["-vk", "ssh"]);
    assert!(options.verbose);
    assert!(options.auto_confirm);
}

#[test]
fn values_can_be_attached_or_separate() {
    assert_eq!(password_argument(&options(&["-pSECRET", "ssh"])), b"SECRET");
    assert_eq!(
        password_argument(&options(&["-p", "SECRET", "ssh"])),
        b"SECRET"
    );
    assert_eq!(
        password_argument(&options(&["-vpSECRET", "ssh"])),
        b"SECRET"
    );
}

#[test]
fn equals_sign_belongs_to_a_short_option_value() {
    assert_eq!(password_argument(&options(&["-p=x", "ssh"])), b"=x");
}

#[test]
fn file_source() {
    match options(&["-f", r"C:\pw.txt", "ssh"]).password {
        PasswordSource::File(path) => assert_eq!(path, Path::new(r"C:\pw.txt")),
        other => panic!("expected -f, got {other:?}"),
    }
}

#[test]
fn handle_accepts_decimal_and_hex() {
    for (text, expected) in [("12", 12), ("0x1C", 28), ("0X1c", 28)] {
        match options(&["-d", text, "ssh"]).password {
            PasswordSource::Handle(handle) => assert_eq!(handle.get(), expected, "{text}"),
            other => panic!("expected -d, got {other:?}"),
        }
    }
}

#[test]
fn handle_rejects_zero_and_garbage() {
    for text in ["0", "0x0", "abc", "-1", ""] {
        assert_eq!(
            error(&["-d", text, "ssh"]),
            UsageError::InvalidHandle(text.to_owned())
        );
    }
}

#[test]
fn env_source() {
    assert!(matches!(
        options(&["-e", "ssh"]).password,
        PasswordSource::Env
    ));
}

#[test]
fn custom_prompt() {
    assert_eq!(options(&["-P", "Passcode:", "ssh"]).prompt, "Passcode:");
}

#[test]
fn empty_prompt_is_rejected() {
    assert_eq!(error(&["-P", "", "ssh"]), UsageError::EmptyPrompt);
}

#[test]
fn long_options() {
    assert!(options(&["--auto-confirm", "ssh"]).auto_confirm);
    assert!(matches!(parse(&["--help"]), Ok(Invocation::Help)));
}

#[test]
fn help_is_answered_as_soon_as_it_is_seen() {
    for args in [&["-h"][..], &["-vh"], &["-v", "--help", "-x"]] {
        assert!(matches!(parse(args), Ok(Invocation::Help)), "{args:?}");
    }
}

#[test]
fn only_one_password_source_is_allowed() {
    for args in [
        &["-p", "a", "-e", "ssh"][..],
        &["-e", "-e", "ssh"],
        &["-f", "x", "-d", "3", "ssh"],
        &["-ep", "a", "ssh"],
    ] {
        assert_eq!(error(args), UsageError::ConflictingSource, "{args:?}");
    }
}

#[test]
fn usage_errors() {
    assert_eq!(error(&[]), UsageError::MissingCommand);
    assert_eq!(error(&["-v"]), UsageError::MissingCommand);
    assert_eq!(error(&["--"]), UsageError::MissingCommand);
    assert_eq!(
        error(&["-x", "ssh"]),
        UsageError::UnknownOption("-x".into())
    );
    assert_eq!(
        error(&["-vx", "ssh"]),
        UsageError::UnknownOption("-x".into())
    );
    assert_eq!(
        error(&["--nope", "ssh"]),
        UsageError::UnknownOption("--nope".into())
    );
    assert_eq!(error(&["-p"]), UsageError::MissingValue('p'));
}

#[test]
fn usage_lists_every_option() {
    let usage = Usage.to_string();
    for option in [
        "-f",
        "-d",
        "-p",
        "-e",
        "-P",
        "-v",
        "-k",
        "--auto-confirm",
        "-h",
        "--help",
    ] {
        assert!(usage.contains(option), "{option} missing from usage");
    }
    assert!(usage.contains(DEFAULT_PROMPT));
}
