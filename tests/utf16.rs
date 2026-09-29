use sshpass::utf16::Utf16Decoder;

fn decode_pieces(pieces: &[&[u16]]) -> String {
    let mut decoder = Utf16Decoder::default();
    let mut text = String::new();
    for piece in pieces {
        decoder.decode(piece, &mut text);
    }
    text
}

#[test]
fn ascii_and_bmp_text() {
    let units: Vec<u16> = "ls -la \u{e9}\u{4e2d}".encode_utf16().collect();
    assert_eq!(decode_pieces(&[&units]), "ls -la \u{e9}\u{4e2d}");
}

#[test]
fn surrogate_pair_in_one_piece() {
    assert_eq!(decode_pieces(&[&[0xD83D, 0xDE00]]), "\u{1F600}");
}

#[test]
fn surrogate_pair_split_across_pieces() {
    assert_eq!(
        decode_pieces(&[&[u16::from(b'a'), 0xD83D], &[0xDE00, u16::from(b'b')]]),
        "a\u{1F600}b"
    );
}

#[test]
fn empty_piece_keeps_the_pending_half() {
    assert_eq!(decode_pieces(&[&[0xD83D], &[], &[0xDE00]]), "\u{1F600}");
}

#[test]
fn unpaired_surrogates_become_replacement_characters() {
    assert_eq!(decode_pieces(&[&[0xDE00]]), "\u{FFFD}");
    assert_eq!(decode_pieces(&[&[0xD83D], &[u16::from(b'x')]]), "\u{FFFD}x");
    assert_eq!(
        decode_pieces(&[&[0xD83D], &[0xD83D, 0xDE00]]),
        "\u{FFFD}\u{1F600}"
    );
}
