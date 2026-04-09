/// Color representation for terminal cells.
///
/// `Named` covers the standard 8/16 ANSI colours (indices 0–15) referenced by
/// name in SGR sequences.  `Indexed` covers the 256-colour palette (0–255).
/// Both are stored identically in [`PackedStyle`] and decoded as
/// [`Color::Indexed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// The terminal's default foreground or background colour.
    Default,
    /// One of the 8/16 standard ANSI colours, addressed by index (0–15).
    /// Stored as and decoded from [`Color::Indexed`].
    Named(u8),
    /// 256-colour palette index (0–255).
    Indexed(u8),
    /// 24-bit true colour.
    Rgb(u8, u8, u8),
}

bitflags::bitflags! {
    /// SGR (Select Graphic Rendition) text-attribute flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::style::Attr;
    /// let a = Attr::BOLD | Attr::ITALIC;
    /// assert!(a.contains(Attr::BOLD));
    /// assert!(!a.contains(Attr::UNDERLINE));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Attr: u16 {
        /// Bold / increased intensity.
        const BOLD             = 0b0000_0000_0001;
        /// Dim / decreased intensity.
        const DIM              = 0b0000_0000_0010;
        /// Italic text.
        const ITALIC           = 0b0000_0000_0100;
        /// Standard underline.
        const UNDERLINE        = 0b0000_0000_1000;
        /// Blinking text.
        const BLINK            = 0b0000_0001_0000;
        /// Reverse video (swap fg/bg).
        const INVERSE          = 0b0000_0010_0000;
        /// Concealed (invisible) text.
        const HIDDEN           = 0b0000_0100_0000;
        /// Strikethrough text.
        const STRIKETHROUGH    = 0b0000_1000_0000;
        /// Curly / wavy underline (Kitty / iTerm extension).
        const CURLY_UNDERLINE  = 0b0001_0000_0000;
        /// Dotted underline.
        const DOTTED_UNDERLINE = 0b0010_0000_0000;
        /// Dashed underline.
        const DASHED_UNDERLINE = 0b0100_0000_0000;
        /// Hyperlink active (OSC 8).
        const HYPERLINK        = 0b1000_0000_0000;
    }
}

/// Packed 8-byte lossless representation of a cell's foreground colour,
/// background colour, and attribute flags.
///
/// # Bit layout (64 bits total, stored as `u64`)
///
/// ```text
/// bits  0– 1  foreground colour type (TAG_DEFAULT=0, TAG_INDEXED=1, TAG_RGB=2)
/// bits  2– 9  foreground colour data (index byte for Indexed, r for Rgb)
/// bits 10–17  foreground colour data (g for Rgb, 0 otherwise)
/// bits 18–25  foreground colour data (b for Rgb, 0 otherwise)
/// bits 26–27  background colour type
/// bits 28–35  background colour data (index / r)
/// bits 36–43  background colour data (g)
/// bits 44–51  background colour data (b)
/// bits 52–63  attribute flags (12 bits used, 4 unused)
/// ```
///
/// This scheme stores all 16 777 216 RGB values, all 256 indexed values, and
/// the default colour losslessly within 8 bytes.
///
/// [`Color::Named`] is stored and decoded as [`Color::Indexed`] because both
/// reference the same colour palette.
///
/// # Size guarantee
///
/// ```
/// use teamucks_vte::style::PackedStyle;
/// const _: () = assert!(std::mem::size_of::<PackedStyle>() <= 8);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackedStyle(u64);

const _: () = assert!(std::mem::size_of::<PackedStyle>() <= 8);

// Colour-type tag values used in the 2-bit tag fields.
const TAG_DEFAULT: u64 = 0;
const TAG_INDEXED: u64 = 1;
const TAG_RGB: u64 = 2;

// Bit positions for foreground colour.
const FG_TAG_SHIFT: u32 = 0;
const FG_DATA0_SHIFT: u32 = 2; // index or r
const FG_DATA1_SHIFT: u32 = 10; // g
const FG_DATA2_SHIFT: u32 = 18; // b

// Bit positions for background colour.
const BG_TAG_SHIFT: u32 = 26;
const BG_DATA0_SHIFT: u32 = 28; // index or r
const BG_DATA1_SHIFT: u32 = 36; // g
const BG_DATA2_SHIFT: u32 = 44; // b

// Bit position for attribute flags.
const ATTR_SHIFT: u32 = 52;

// Masks used to clear and read the 2-bit tag and 8-bit data fields.
const TAG_MASK: u64 = 0b11;
const DATA_MASK: u64 = 0xFF;
const ATTR_MASK: u64 = 0xFFF;

impl PackedStyle {
    /// Encode a [`Color`] into its tag (2 bits) and three data bytes.
    ///
    /// Returns `(tag, d0, d1, d2)` where each value fits in its declared bit
    /// width (2 bits for tag, 8 bits each for d0/d1/d2).
    #[inline]
    fn encode_color(color: Color) -> (u64, u64, u64, u64) {
        match color {
            Color::Default => (TAG_DEFAULT, 0, 0, 0),
            // Named and Indexed share the palette — store identically.
            Color::Named(idx) | Color::Indexed(idx) => (TAG_INDEXED, u64::from(idx), 0, 0),
            Color::Rgb(r, g, b) => (TAG_RGB, u64::from(r), u64::from(g), u64::from(b)),
        }
    }

    /// Decode tag and three data bytes into a [`Color`].
    ///
    /// `d0`, `d1`, `d2` are extracted with `DATA_MASK` so each is at most
    /// 0xFF; the truncating casts below are therefore safe.
    #[inline]
    fn decode_color(tag: u64, d0: u64, d1: u64, d2: u64) -> Color {
        match tag {
            TAG_INDEXED => {
                // d0 is masked to 8 bits — cast is safe.
                #[allow(clippy::cast_possible_truncation)]
                Color::Indexed(d0 as u8)
            }
            TAG_RGB => {
                // d0, d1, d2 are each masked to 8 bits — casts are safe.
                #[allow(clippy::cast_possible_truncation)]
                Color::Rgb(d0 as u8, d1 as u8, d2 as u8)
            }
            _ => Color::Default,
        }
    }

    /// Write an encoded colour into `self` at the given bit positions.
    #[inline]
    fn set_color_bits(
        &mut self,
        tag_shift: u32,
        d0_shift: u32,
        d1_shift: u32,
        d2_shift: u32,
        color: Color,
    ) {
        let (tag, d0, d1, d2) = Self::encode_color(color);
        // Clear existing bits for this colour slot.
        let clear_mask = (TAG_MASK << tag_shift)
            | (DATA_MASK << d0_shift)
            | (DATA_MASK << d1_shift)
            | (DATA_MASK << d2_shift);
        self.0 &= !clear_mask;
        // Write new bits.
        self.0 |= (tag & TAG_MASK) << tag_shift;
        self.0 |= (d0 & DATA_MASK) << d0_shift;
        self.0 |= (d1 & DATA_MASK) << d1_shift;
        self.0 |= (d2 & DATA_MASK) << d2_shift;
    }

    /// Read a colour from `self` at the given bit positions.
    ///
    /// Takes `self` by value because `PackedStyle` is `Copy` (8 bytes) and
    /// passing by value avoids an indirect memory access.
    #[inline]
    fn get_color_bits(self, tag_shift: u32, d0_shift: u32, d1_shift: u32, d2_shift: u32) -> Color {
        let tag = (self.0 >> tag_shift) & TAG_MASK;
        let d0 = (self.0 >> d0_shift) & DATA_MASK;
        let d1 = (self.0 >> d1_shift) & DATA_MASK;
        let d2 = (self.0 >> d2_shift) & DATA_MASK;
        Self::decode_color(tag, d0, d1, d2)
    }

    /// Return the foreground [`Color`].
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::style::{Color, PackedStyle};
    /// let s = PackedStyle::default();
    /// assert_eq!(s.foreground(), Color::Default);
    /// ```
    #[must_use]
    pub fn foreground(self) -> Color {
        self.get_color_bits(FG_TAG_SHIFT, FG_DATA0_SHIFT, FG_DATA1_SHIFT, FG_DATA2_SHIFT)
    }

    /// Return the background [`Color`].
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::style::{Color, PackedStyle};
    /// let s = PackedStyle::default();
    /// assert_eq!(s.background(), Color::Default);
    /// ```
    #[must_use]
    pub fn background(self) -> Color {
        self.get_color_bits(BG_TAG_SHIFT, BG_DATA0_SHIFT, BG_DATA1_SHIFT, BG_DATA2_SHIFT)
    }

    /// Set the foreground colour.
    pub fn set_foreground(&mut self, color: Color) {
        self.set_color_bits(FG_TAG_SHIFT, FG_DATA0_SHIFT, FG_DATA1_SHIFT, FG_DATA2_SHIFT, color);
    }

    /// Set the background colour.
    pub fn set_background(&mut self, color: Color) {
        self.set_color_bits(BG_TAG_SHIFT, BG_DATA0_SHIFT, BG_DATA1_SHIFT, BG_DATA2_SHIFT, color);
    }

    /// Return all attribute flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::style::{Attr, PackedStyle};
    /// let s = PackedStyle::default();
    /// assert_eq!(s.attrs(), Attr::empty());
    /// ```
    #[must_use]
    pub fn attrs(self) -> Attr {
        let bits = (self.0 >> ATTR_SHIFT) & ATTR_MASK;
        // ATTR_MASK is 0xFFF which fits in u16; cast is safe.
        #[allow(clippy::cast_possible_truncation)]
        Attr::from_bits_retain(bits as u16)
    }

    /// Return `true` if *all* bits of `attr` are set.
    #[must_use]
    pub fn has_attr(self, attr: Attr) -> bool {
        self.attrs().contains(attr)
    }

    /// Set (enable) the given attribute bits.
    pub fn set_attr(&mut self, attr: Attr) {
        let current = (self.0 >> ATTR_SHIFT) & ATTR_MASK;
        let new = current | u64::from(attr.bits());
        self.0 = (self.0 & !(ATTR_MASK << ATTR_SHIFT)) | ((new & ATTR_MASK) << ATTR_SHIFT);
    }

    /// Clear (disable) the given attribute bits.
    pub fn clear_attr(&mut self, attr: Attr) {
        let current = (self.0 >> ATTR_SHIFT) & ATTR_MASK;
        let new = current & !u64::from(attr.bits());
        self.0 = (self.0 & !(ATTR_MASK << ATTR_SHIFT)) | ((new & ATTR_MASK) << ATTR_SHIFT);
    }

    /// Reset this style to its default state (default colours, no attributes).
    pub fn reset(&mut self) {
        self.0 = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_all_zeros() {
        let s = PackedStyle::default();
        assert_eq!(s.0, 0);
        assert_eq!(s.foreground(), Color::Default);
        assert_eq!(s.background(), Color::Default);
        assert_eq!(s.attrs(), Attr::empty());
    }

    #[test]
    fn test_rgb_255_0_128_round_trips() {
        let mut s = PackedStyle::default();
        s.set_foreground(Color::Rgb(255, 0, 128));
        assert_eq!(s.foreground(), Color::Rgb(255, 0, 128));
    }

    #[test]
    fn test_all_rgb_values_round_trip() {
        for r in [0u8, 127, 128, 253, 254, 255] {
            for g in [0u8, 127, 255] {
                for b in [0u8, 63, 127, 128, 255] {
                    let mut s = PackedStyle::default();
                    s.set_foreground(Color::Rgb(r, g, b));
                    assert_eq!(
                        s.foreground(),
                        Color::Rgb(r, g, b),
                        "failed for Rgb({r}, {g}, {b})"
                    );
                }
            }
        }
    }
}
