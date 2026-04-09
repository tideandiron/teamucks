/// Character set designators for G0 and G1.
///
/// Terminal character sets determine how printable bytes are mapped to display
/// characters. The default is plain ASCII. DEC Special Graphics provides the
/// box-drawing and symbol characters used by TUI applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Charset {
    /// Plain ASCII — bytes map to their standard Unicode equivalents.
    #[default]
    Ascii,
    /// DEC Special Graphics — bytes in 0x60–0x7E are remapped to box-drawing
    /// and special-symbol Unicode code points. All other bytes map as ASCII.
    DecSpecialGraphics,
}

/// Translate a byte through the DEC Special Graphics character set.
///
/// Returns `Some(c)` when `byte` has a DEC Special Graphics mapping, or `None`
/// when the byte passes through unchanged (use the original character).
///
/// Only the subset actually used by applications is mapped here. The full DEC
/// range is 0x5F–0x7E; entries not listed below pass through as ASCII.
///
/// # Examples
///
/// ```
/// use teamucks_vte::charsets::dec_special_graphics;
///
/// // Horizontal line ('q' -> '─').
/// assert_eq!(dec_special_graphics(b'q'), Some('─'));
/// // Unmapped byte passes through.
/// assert_eq!(dec_special_graphics(b'A'), None);
/// ```
#[must_use]
pub fn dec_special_graphics(byte: u8) -> Option<char> {
    match byte {
        b'`' => Some('\u{25C6}'), // ◆ diamond
        b'a' => Some('\u{2592}'), // ▒ checkerboard / medium shade
        b'f' => Some('\u{00B0}'), // ° degree sign
        b'g' => Some('\u{00B1}'), // ± plus-minus
        b'j' => Some('\u{2518}'), // ┘ lower-right corner
        b'k' => Some('\u{2510}'), // ┐ upper-right corner
        b'l' => Some('\u{250C}'), // ┌ upper-left corner
        b'm' => Some('\u{2514}'), // └ lower-left corner
        b'n' => Some('\u{253C}'), // ┼ crossing / plus
        b'q' => Some('\u{2500}'), // ─ horizontal line
        b't' => Some('\u{251C}'), // ├ left tee
        b'u' => Some('\u{2524}'), // ┤ right tee
        b'v' => Some('\u{2534}'), // ┴ bottom tee
        b'w' => Some('\u{252C}'), // ┬ top tee
        b'x' => Some('\u{2502}'), // │ vertical line
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{dec_special_graphics, Charset};

    #[test]
    fn test_charset_default_is_ascii() {
        assert_eq!(Charset::default(), Charset::Ascii);
    }

    #[test]
    fn test_dec_special_graphics_horizontal_line() {
        assert_eq!(dec_special_graphics(b'q'), Some('─'));
    }

    #[test]
    fn test_dec_special_graphics_vertical_line() {
        assert_eq!(dec_special_graphics(b'x'), Some('│'));
    }

    #[test]
    fn test_dec_special_graphics_corners() {
        assert_eq!(dec_special_graphics(b'l'), Some('┌'));
        assert_eq!(dec_special_graphics(b'k'), Some('┐'));
        assert_eq!(dec_special_graphics(b'm'), Some('└'));
        assert_eq!(dec_special_graphics(b'j'), Some('┘'));
    }

    #[test]
    fn test_dec_special_graphics_tees() {
        assert_eq!(dec_special_graphics(b't'), Some('├'));
        assert_eq!(dec_special_graphics(b'u'), Some('┤'));
        assert_eq!(dec_special_graphics(b'v'), Some('┴'));
        assert_eq!(dec_special_graphics(b'w'), Some('┬'));
    }

    #[test]
    fn test_dec_special_graphics_crossing() {
        assert_eq!(dec_special_graphics(b'n'), Some('┼'));
    }

    #[test]
    fn test_dec_special_graphics_diamond() {
        assert_eq!(dec_special_graphics(b'`'), Some('◆'));
    }

    #[test]
    fn test_dec_special_graphics_degree() {
        assert_eq!(dec_special_graphics(b'f'), Some('°'));
    }

    #[test]
    fn test_dec_special_graphics_plus_minus() {
        assert_eq!(dec_special_graphics(b'g'), Some('±'));
    }

    #[test]
    fn test_dec_special_graphics_unmapped_uppercase() {
        // Uppercase ASCII letters are not in the DEC graphics set.
        assert_eq!(dec_special_graphics(b'A'), None);
        assert_eq!(dec_special_graphics(b'Z'), None);
    }

    #[test]
    fn test_dec_special_graphics_unmapped_digit() {
        assert_eq!(dec_special_graphics(b'0'), None);
    }
}
