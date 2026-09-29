use std::{env, fs, process};

use sshpass::password::{PasswordError, PasswordSource, Secret, read_first_line};

fn first_line(input: &[u8]) -> Vec<u8> {
    read_first_line(input)
        .expect("reading a slice cannot fail")
        .expose()
        .to_vec()
}

#[test]
fn stops_at_the_first_line_break() {
    assert_eq!(first_line(b"secret\nnext"), b"secret");
    assert_eq!(first_line(b"secret\r\nnext"), b"secret");
    assert_eq!(first_line(b"secret\rnext"), b"secret");
}

#[test]
fn takes_everything_without_a_line_break() {
    assert_eq!(first_line(b"secret"), b"secret");
    assert_eq!(first_line(b""), b"");
    assert_eq!(first_line(b"\n"), b"");
}

#[test]
fn long_passwords_survive_buffer_growth() {
    let long: Vec<u8> = (b'a'..=b'z').cycle().take(1000).collect();
    let mut input = long.clone();
    input.extend_from_slice(b"\nrest");
    assert_eq!(first_line(&input), long);
}

#[test]
fn debug_output_hides_the_secret() {
    let secret = Secret::from("hunter2".to_owned());
    assert!(!format!("{secret:?}").contains("hunter2"));
}

#[test]
fn argument_source_resolves_to_itself() {
    let secret = PasswordSource::Argument(Secret::from("pw".to_owned()))
        .resolve()
        .unwrap();
    assert_eq!(secret.expose(), b"pw");
}

#[test]
fn file_source_reads_the_first_line() {
    let path = env::temp_dir().join(format!("sshpass-password-test-{}.txt", process::id()));
    fs::write(&path, "from-file\r\nignored\r\n").unwrap();
    let secret = PasswordSource::File(path.clone()).resolve();
    fs::remove_file(&path).unwrap();
    assert_eq!(secret.unwrap().expose(), b"from-file");
}

#[test]
fn missing_file_reports_its_path() {
    let path = env::temp_dir().join("sshpass-password-test-missing.txt");
    match PasswordSource::File(path.clone()).resolve() {
        Err(error @ PasswordError::File { .. }) => {
            assert!(error.to_string().contains(&*path.to_string_lossy()));
            assert!(std::error::Error::source(&error).is_some());
        }
        other => panic!("expected a file error, got {other:?}"),
    }
}
