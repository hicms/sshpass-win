use sshpass::host_modes::HostModeFilter;

fn filter_chunks(chunks: &[&[u8]]) -> Vec<u8> {
    let mut filter = HostModeFilter::default();
    let mut out = Vec::new();
    for chunk in chunks {
        filter.filter(chunk, &mut out);
    }
    filter.finish(&mut out);
    out
}

#[test]
fn mode_switches_are_removed() {
    assert_eq!(
        filter_chunks(&[b"a\x1b[?9001h\x1b[?1004hb\r\n\x1b[?9001l\x1b[?1004l"]),
        b"ab\r\n"
    );
}

#[test]
fn switch_split_across_chunks_is_removed() {
    let mut filter = HostModeFilter::default();
    let mut out = Vec::new();
    filter.filter(b"x\x1b[?90", &mut out);
    assert_eq!(out, b"x", "a possible switch is held back");
    filter.filter(b"01ly", &mut out);
    assert_eq!(out, b"xy");
}

#[test]
fn other_escape_sequences_pass() {
    let colored: &[u8] = b"\x1b[?25l\x1b[31mred\x1b[0m\x1b[?1049h\x1b]0;title\x07";
    assert_eq!(filter_chunks(&[colored]), colored);
}

#[test]
fn lookalikes_pass() {
    assert_eq!(filter_chunks(&[b"\x1b[?9002h"]), b"\x1b[?9002h");
    assert_eq!(filter_chunks(&[b"\x1b[?1004;1h"]), b"\x1b[?1004;1h");
    assert_eq!(filter_chunks(&[b"\x1b\x1b[?1004h"]), b"\x1b");
}

#[test]
fn unfinished_switch_is_kept_at_the_end() {
    assert_eq!(filter_chunks(&[b"end\x1b[?900"]), b"end\x1b[?900");
}
