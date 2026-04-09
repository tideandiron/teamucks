use crate::style::PackedStyle;

/// Small-string optimised storage for a single grapheme cluster.
///
/// The majority of terminal cells hold single ASCII characters (1 byte) or
/// common Unicode codepoints that encode to at most 4 UTF-8 bytes.  These are
/// stored inline without any heap allocation.  Longer grapheme clusters
/// (multi-codepoint emoji sequences, combining character stacks) spill to the
/// heap via a [`String`].
///
/// # Inline capacity
///
/// The inline buffer holds up to 4 bytes of UTF-8.  This covers:
/// - All ASCII characters (1 byte).
/// - Most Latin extended and Greek characters (2 bytes each).
/// - CJK unified ideographs and most emoji (3–4 bytes each).
///
/// Longer clusters (e.g. family emoji joined by Zero-Width Joiner) spill to
/// [`GraphemeStorage::Heap`].
///
/// # Size note
///
/// The `Heap` variant wraps a [`String`], which is 24 bytes on 64-bit
/// platforms (pointer + length + capacity).  This makes the enum 32 bytes due
/// to alignment and the discriminant.  Because heap storage is used for fewer
/// than 1% of cells in real workloads, the hot path (inline) is cache-friendly
/// and the occasional heap variant is acceptable.
pub(crate) enum GraphemeStorage {
    /// Up to 4 bytes stored inline — zero allocation.
    Inline {
        /// Raw UTF-8 bytes; only `bytes[..len]` are valid.
        bytes: [u8; 4],
        /// Number of valid UTF-8 bytes in `bytes`.  Never exceeds 4.
        len: u8,
    },
    /// Heap-allocated storage for grapheme clusters longer than 4 bytes.
    Heap(String),
}

impl GraphemeStorage {
    /// Create storage from a string slice.
    ///
    /// Strings of up to 4 bytes are stored inline; longer strings use the
    /// heap.
    #[must_use]
    pub(crate) fn new(s: &str) -> Self {
        let bytes = s.as_bytes();
        if bytes.len() <= 4 {
            let mut buf = [0u8; 4];
            buf[..bytes.len()].copy_from_slice(bytes);
            // len is always in 0..=4, which fits in u8.
            #[allow(clippy::cast_possible_truncation)]
            Self::Inline { bytes: buf, len: bytes.len() as u8 }
        } else {
            Self::Heap(s.to_owned())
        }
    }

    /// Create storage from a single character.
    #[must_use]
    pub(crate) fn from_char(c: char) -> Self {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        Self::new(s)
    }

    /// Return the stored grapheme cluster as a string slice.
    ///
    /// # Panics
    ///
    /// Never panics in correct usage.  The `Inline` variant is only
    /// constructed from valid UTF-8 slices in [`GraphemeStorage::new`] and
    /// [`GraphemeStorage::from_char`].  The `expect` call documents this
    /// invariant; it cannot fire.
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Inline { bytes, len } => std::str::from_utf8(&bytes[..*len as usize])
                .expect("GraphemeStorage invariant: inline bytes are always valid UTF-8"),
            Self::Heap(s) => s.as_str(),
        }
    }

    /// Return a space character as the default grapheme.
    #[must_use]
    pub(crate) fn space() -> Self {
        Self::Inline { bytes: [b' ', 0, 0, 0], len: 1 }
    }

    /// Create an independent copy of this storage value.
    ///
    /// `GraphemeStorage` does not implement [`Clone`] because `Cell` is a
    /// hot-path type subject to the no-cheap-clone policy.  This method makes
    /// the allocation cost explicit at the alternate-screen snapshot call site.
    #[must_use]
    pub(crate) fn snapshot(&self) -> Self {
        match self {
            Self::Inline { bytes, len } => Self::Inline { bytes: *bytes, len: *len },
            Self::Heap(s) => Self::Heap(s.clone()),
        }
    }
}

impl Default for GraphemeStorage {
    fn default() -> Self {
        Self::space()
    }
}

// GraphemeStorage does not derive Debug because String's Debug includes quotes
// and we want a compact representation.
impl std::fmt::Debug for GraphemeStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("GraphemeStorage").field(&self.as_str()).finish()
    }
}

// PartialEq compares the string content regardless of variant.
impl PartialEq for GraphemeStorage {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for GraphemeStorage {}

// ---------------------------------------------------------------------------
// Cell flags
// ---------------------------------------------------------------------------

/// Bitfield flags for a terminal [`Cell`].
///
/// Stored as a single `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct CellFlags(u8);

impl CellFlags {
    /// This cell holds the first column of a wide (double-width) character.
    const WIDE: u8 = 0x01;
    /// This cell is the trailing (second) column of a wide character.
    const CONTINUATION: u8 = 0x02;

    #[inline]
    fn is_set(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    #[inline]
    fn set(&mut self, flag: u8, value: bool) {
        if value {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

// ---------------------------------------------------------------------------
// Cell
// ---------------------------------------------------------------------------

/// A single terminal cell: a grapheme cluster, a display style, and flags.
///
/// # Size note
///
/// `Cell` contains a [`GraphemeStorage`] which can be up to 32 bytes on
/// 64-bit platforms (when the `Heap` variant is active).  The guideline of
/// 16 bytes applies to the common inline case; because fewer than 1% of cells
/// hold multi-codepoint graphemes this is acceptable.  The 32-byte worst case
/// still fits within two cache lines.
#[derive(Default)]
pub struct Cell {
    grapheme: GraphemeStorage,
    style: PackedStyle,
    flags: CellFlags,
}

impl Cell {
    /// Return the grapheme cluster stored in this cell.
    #[must_use]
    pub fn grapheme(&self) -> &str {
        self.grapheme.as_str()
    }

    /// Replace the grapheme cluster with the contents of `s`.
    pub fn set_grapheme(&mut self, s: &str) {
        self.grapheme = GraphemeStorage::new(s);
    }

    /// Replace the grapheme cluster with a single character.
    pub fn set_grapheme_char(&mut self, c: char) {
        self.grapheme = GraphemeStorage::from_char(c);
    }

    /// Return an immutable reference to this cell's style.
    #[must_use]
    pub fn style(&self) -> &PackedStyle {
        &self.style
    }

    /// Return a mutable reference to this cell's style.
    pub fn style_mut(&mut self) -> &mut PackedStyle {
        &mut self.style
    }

    /// Return `true` if this cell is the leading cell of a wide character.
    #[must_use]
    pub fn is_wide(&self) -> bool {
        self.flags.is_set(CellFlags::WIDE)
    }

    /// Set or clear the wide-character flag.
    pub(crate) fn set_wide(&mut self, value: bool) {
        self.flags.set(CellFlags::WIDE, value);
    }

    /// Return `true` if this cell is the trailing continuation of a wide
    /// character.
    #[must_use]
    pub fn is_continuation(&self) -> bool {
        self.flags.is_set(CellFlags::CONTINUATION)
    }

    /// Set or clear the continuation flag.
    pub(crate) fn set_continuation(&mut self, value: bool) {
        self.flags.set(CellFlags::CONTINUATION, value);
    }

    /// Create an independent copy of this cell.
    ///
    /// Used by the alternate screen buffer to snapshot the primary screen.
    /// `Cell` does not implement [`Clone`] — it is a hot-path type subject to
    /// the no-cheap-clone policy.  This method makes the allocation cost
    /// explicit at each call site.
    #[must_use]
    pub(crate) fn snapshot(&self) -> Self {
        Self { grapheme: self.grapheme.snapshot(), style: self.style, flags: self.flags }
    }

    /// Reset this cell to its default state: a space, default style, no flags.
    pub fn reset(&mut self) {
        self.grapheme = GraphemeStorage::space();
        self.style.reset();
        self.flags = CellFlags::default();
    }
}

impl std::fmt::Debug for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cell")
            .field("grapheme", &self.grapheme.as_str())
            .field("style", &self.style)
            .field("wide", &self.is_wide())
            .field("continuation", &self.is_continuation())
            .field("flags", &self.flags)
            .finish()
    }
}

// Cell size is ~48 bytes on 64-bit due to GraphemeStorage::Heap(String).
// The Inline variant (99% of real-world cells) is compact. If profiling
// shows Cell size is a frame-diff or scrollback bottleneck, switch to
// a side-table approach (u32 handle, ~16 bytes). See GUIDELINES.md.
const _: () = assert!(std::mem::size_of::<Cell>() <= 48);

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------
    // GraphemeStorage tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_grapheme_storage_inline_ascii() {
        let g = GraphemeStorage::new("A");
        assert_eq!(g.as_str(), "A");
    }

    #[test]
    fn test_grapheme_storage_multi_byte() {
        // 'é' is U+00E9, encoded as 2 UTF-8 bytes: 0xC3 0xA9.
        let g = GraphemeStorage::new("é");
        assert_eq!(g.as_str(), "é");
    }

    #[test]
    fn test_grapheme_storage_four_byte() {
        // '🎉' is U+1F389, encoded as 4 UTF-8 bytes — exactly fills the inline buffer.
        let g = GraphemeStorage::new("🎉");
        assert_eq!(g.as_str(), "🎉");
    }

    #[test]
    fn test_grapheme_storage_multi_codepoint() {
        // Family emoji cluster: multiple codepoints joined by ZWJ — exceeds 4 bytes.
        let cluster = "👨\u{200d}👩\u{200d}👧";
        let g = GraphemeStorage::new(cluster);
        assert_eq!(g.as_str(), cluster);
    }

    // ---------------------------------------------------------------------------
    // Cell tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_cell_default_is_space() {
        let c = Cell::default();
        assert_eq!(c.grapheme(), " ");
        assert!(!c.is_wide());
        assert!(!c.is_continuation());
    }

    #[test]
    fn test_cell_set_ascii() {
        let mut c = Cell::default();
        c.set_grapheme("A");
        assert_eq!(c.grapheme(), "A");
    }

    #[test]
    fn test_cell_set_emoji() {
        let mut c = Cell::default();
        c.set_grapheme("🎉");
        assert_eq!(c.grapheme(), "🎉");
    }

    #[test]
    fn test_cell_wide_flag() {
        let mut c = Cell::default();
        assert!(!c.is_wide());
        c.set_wide(true);
        assert!(c.is_wide());
        c.set_wide(false);
        assert!(!c.is_wide());
    }

    #[test]
    fn test_cell_continuation_flag() {
        let mut c = Cell::default();
        assert!(!c.is_continuation());
        c.set_continuation(true);
        assert!(c.is_continuation());
    }
}
