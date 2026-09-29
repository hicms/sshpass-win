use sshpass::cmdline::CommandLine;

fn line(program: &str, args: &[&str]) -> String {
    CommandLine::new(program, args).to_string()
}

#[test]
fn plain_arguments_are_left_alone() {
    assert_eq!(
        line("ssh", &["user@host", "ls", "-la"]),
        "ssh user@host ls -la"
    );
}

#[test]
fn whitespace_forces_quotes() {
    assert_eq!(
        line("rsync", &["-e", "ssh -p 2222"]),
        r#"rsync -e "ssh -p 2222""#
    );
    assert_eq!(line("x", &["a\tb"]), "x \"a\tb\"");
}

#[test]
fn empty_argument_is_kept() {
    assert_eq!(line("x", &["", "y"]), r#"x "" y"#);
}

#[test]
fn quotes_are_escaped() {
    assert_eq!(line("x", &[r#"a"b"#]), r#"x "a\"b""#);
}

#[test]
fn backslashes_are_literal_unless_before_a_quote() {
    assert_eq!(line("x", &[r"C:\dir\file"]), r"x C:\dir\file");
    assert_eq!(line("x", &[r"C:\my dir\file"]), r#"x "C:\my dir\file""#);
    assert_eq!(line("x", &[r"C:\my dir\"]), r#"x "C:\my dir\\""#);
    assert_eq!(line("x", &[r#"a\"b"#]), r#"x "a\\\"b""#);
}

#[test]
fn program_with_spaces_is_quoted() {
    assert_eq!(
        line(r"C:\Program Files\OpenSSH\ssh.exe", &["host"]),
        r#""C:\Program Files\OpenSSH\ssh.exe" host"#
    );
}
