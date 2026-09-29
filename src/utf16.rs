//! Incremental UTF-16 to UTF-8 conversion for console input.

/// Converts UTF-16 that arrives in arbitrary pieces.
///
/// A surrogate pair can be split across two console reads; the high half is
/// held back until the next piece instead of being replaced with U+FFFD.
#[derive(Debug, Default)]
pub struct Utf16Decoder {
    pending_high: Option<u16>,
}

impl Utf16Decoder {
    /// Appends the text of `units` to `out`; unpaired surrogates become U+FFFD.
    pub fn decode(&mut self, units: &[u16], out: &mut String) {
        let Some((&last, rest)) = units.split_last() else {
            return;
        };
        let (complete, carry) = if is_high_surrogate(last) {
            (rest, Some(last))
        } else {
            (units, None)
        };
        let pending = self.pending_high.take();
        out.extend(
            char::decode_utf16(pending.into_iter().chain(complete.iter().copied()))
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)),
        );
        self.pending_high = carry;
    }
}

const fn is_high_surrogate(unit: u16) -> bool {
    matches!(unit, 0xD800..=0xDBFF)
}
