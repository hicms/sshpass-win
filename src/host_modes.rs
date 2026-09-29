//! Terminal mode switches the pseudo console addresses to sshpass itself.

/// The pseudo console asks its host terminal for win32-input-mode (9001) and
/// focus reporting (1004) when it opens, and switches them off when it
/// closes. sshpass is that host and uses neither. Passed on, they would end
/// up in redirected output or reset modes that the user's terminal enabled
/// for its own console.
const SWITCHES: [&[u8]; 4] = [
    b"\x1b[?9001h",
    b"\x1b[?9001l",
    b"\x1b[?1004h",
    b"\x1b[?1004l",
];

/// Removes [`SWITCHES`] from a byte stream, including switches split across
/// chunks.
#[derive(Debug, Default)]
pub struct HostModeFilter {
    /// A prefix of a switch, held back until the next bytes decide it.
    pending: Vec<u8>,
}

impl HostModeFilter {
    /// Appends `bytes` without mode switches to `out`.
    pub fn filter(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        for &byte in bytes {
            // Switches contain ESC only as their first byte, so an ESC always
            // ends the pending candidate and may start a new one.
            if byte == b'\x1b' {
                out.append(&mut self.pending);
            } else if self.pending.is_empty() {
                out.push(byte);
                continue;
            }
            self.pending.push(byte);
            if SWITCHES.contains(&self.pending.as_slice()) {
                self.pending.clear();
            } else if !SWITCHES
                .iter()
                .any(|switch| switch.starts_with(&self.pending))
            {
                out.append(&mut self.pending);
            }
        }
    }

    /// Appends whatever is still held back, for the end of the stream.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        out.append(&mut self.pending);
    }
}
