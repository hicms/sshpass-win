use sshpass::prompt::{Detector, Failure, Reaction};

const PROMPT: &[u8] = b"user@host's password: ";

fn logged_in_detector() -> Detector {
    let mut detector = Detector::new("password:", false);
    assert_eq!(detector.feed(PROMPT), Reaction::SendPassword);
    detector
}

#[test]
fn output_before_the_prompt_is_hidden() {
    let mut detector = Detector::new("password:", false);
    assert_eq!(detector.feed(b"banner\r\n"), Reaction::Hide(b"banner\r\n"));
    assert_eq!(detector.feed(PROMPT), Reaction::SendPassword);
}

#[test]
fn prompt_split_across_chunks_is_found() {
    let mut detector = Detector::new("password:", false);
    assert_eq!(
        detector.feed(b"user@host's pass"),
        Reaction::Hide(b"user@host's pass")
    );
    assert_eq!(detector.feed(b"word: "), Reaction::SendPassword);
}

#[test]
fn prompt_overlapping_a_partial_match_is_found() {
    let mut detector = Detector::new("aab", false);
    assert_eq!(detector.feed(b"aa"), Reaction::Hide(b"aa"));
    assert_eq!(detector.feed(b"ab"), Reaction::SendPassword);
}

#[test]
fn prompt_is_case_sensitive() {
    let mut detector = Detector::new("password:", false);
    assert_eq!(detector.feed(b"Password: "), Reaction::Hide(b"Password: "));
}

#[test]
fn output_after_login_is_forwarded() {
    let mut detector = logged_in_detector();
    assert_eq!(detector.feed(b"\r\n"), Reaction::Hide(b"\r\n"));
    for chunk in [
        &b"Last login: today\r\n"[..],
        b"\r\n",
        b"Welcome\r\n",
        b"$ ",
    ] {
        assert_eq!(detector.feed(chunk), Reaction::Forward(chunk));
    }
    assert_eq!(
        detector.feed(PROMPT),
        Reaction::Forward(PROMPT),
        "remote prompts are not ours"
    );
}

#[test]
fn enter_echo_ending_the_prompt_line_is_hidden() {
    let mut detector = logged_in_detector();
    assert_eq!(
        detector.feed(b"\x1b[?25h\r\n\r\nline1\r\n"),
        Reaction::Forward(b"\r\nline1\r\n"),
        "only the prompt's own line is hidden"
    );
}

#[test]
fn enter_echo_split_across_chunks_is_hidden() {
    let mut detector = logged_in_detector();
    assert_eq!(detector.feed(b"\r"), Reaction::Hide(b"\r"));
    assert_eq!(detector.feed(b"\nline1"), Reaction::Forward(b"line1"));
}

#[test]
fn text_on_the_prompt_line_is_shown() {
    let mut detector = logged_in_detector();
    assert_eq!(detector.feed(b"ok\r\n"), Reaction::Forward(b"ok\r\n"));
    assert_eq!(detector.feed(b"\r\n"), Reaction::Forward(b"\r\n"));
}

#[test]
fn prompt_after_the_retry_message_means_incorrect_password() {
    let mut detector = logged_in_detector();
    let message = b"Permission denied, please try again.\r\n";
    assert_eq!(detector.feed(b"\r\n"), Reaction::Hide(b"\r\n"));
    assert_eq!(detector.feed(message), Reaction::Forward(message));
    assert_eq!(
        detector.feed(PROMPT),
        Reaction::Fail(Failure::IncorrectPassword)
    );
    assert_eq!(
        detector.feed(b"more"),
        Reaction::Hide(b"more"),
        "drained after failure"
    );
}

#[test]
fn blank_lines_and_escape_sequences_are_not_text() {
    let mut detector = logged_in_detector();
    let echo = b"\x1b[?25l\r\n";
    let noise = b"\x1b]0;C:\\ssh.exe\x07\r\n\x1b[2;1H\x1b(B\r\n";
    assert_eq!(detector.feed(echo), Reaction::Hide(echo));
    assert_eq!(detector.feed(noise), Reaction::Forward(noise));
    assert_eq!(
        detector.feed(b"Permission denied\r\n"),
        Reaction::Forward(b"Permission denied\r\n")
    );
    assert_eq!(
        detector.feed(PROMPT),
        Reaction::Fail(Failure::IncorrectPassword)
    );
}

#[test]
fn escape_sequence_split_across_chunks_is_not_text() {
    let mut detector = logged_in_detector();
    detector.feed(b"\x1b[");
    detector.feed(b"?25h\r\n");
    detector.feed(b"Permission denied\r\n");
    assert_eq!(
        detector.feed(PROMPT),
        Reaction::Fail(Failure::IncorrectPassword)
    );
}

#[test]
fn unknown_host_key_fails_without_auto_confirm() {
    let mut detector = Detector::new("password:", false);
    let question = b"Are you sure you want to continue connecting (yes/no/[fingerprint])? ";
    assert_eq!(
        detector.feed(question),
        Reaction::Fail(Failure::HostKeyUnknown)
    );
}

#[test]
fn unknown_host_key_is_confirmed_with_auto_confirm() {
    let mut detector = Detector::new("password:", true);
    assert_eq!(
        detector.feed(b"Are you sure you want to continue connecting (yes/no)? "),
        Reaction::ConfirmHostKey
    );
    assert_eq!(detector.feed(PROMPT), Reaction::SendPassword);
}

#[test]
fn changed_ip_key_fails_even_with_auto_confirm() {
    let mut detector = Detector::new("password:", true);
    let warning = b"Warning: the ECDSA host key for 'h' differs from the key for the IP address '10.0.0.1'\r\n\
        Are you sure you want to continue connecting (yes/no)? ";
    assert_eq!(
        detector.feed(warning),
        Reaction::Fail(Failure::HostKeyChanged)
    );
}

#[test]
fn failures_explain_themselves() {
    assert_eq!(Failure::IncorrectPassword.to_string(), "incorrect password");
    assert!(Failure::HostKeyUnknown.to_string().contains("-k"));
}
