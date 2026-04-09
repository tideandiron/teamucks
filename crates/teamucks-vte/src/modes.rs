use bitflags::bitflags;

bitflags! {
    /// Terminal mode flags controlled by DECSET/DECRST (`CSI ? h` / `CSI ? l`)
    /// and standard set/reset (`CSI h` / `CSI l`).
    ///
    /// The default state (all bits zero) does **not** represent a valid
    /// terminal configuration.  Use [`ModeFlags::default_modes`] to obtain the
    /// correct initial value.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::modes::ModeFlags;
    ///
    /// let m = ModeFlags::default_modes();
    /// assert!(m.contains(ModeFlags::AUTO_WRAP));
    /// assert!(m.contains(ModeFlags::CURSOR_VISIBLE));
    /// assert!(!m.contains(ModeFlags::ORIGIN));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ModeFlags: u32 {
        /// DECCKM — Application cursor keys.
        const CURSOR_KEYS_APPLICATION = 1 << 0;
        /// DECOM — Origin mode (cursor relative to scroll region).
        const ORIGIN = 1 << 1;
        /// DECAWM — Auto-wrap mode.
        const AUTO_WRAP = 1 << 2;
        /// DECTCEM — Cursor visible.
        const CURSOR_VISIBLE = 1 << 3;
        /// Mode 1000 — Basic mouse reporting.
        const MOUSE_REPORT_CLICK = 1 << 4;
        /// Mode 1002 — Button event mouse tracking.
        const MOUSE_REPORT_BUTTON = 1 << 5;
        /// Mode 1003 — All motion mouse tracking.
        const MOUSE_REPORT_ALL = 1 << 6;
        /// Mode 1006 — SGR mouse format.
        const MOUSE_SGR_FORMAT = 1 << 7;
        /// Mode 2004 — Bracketed paste.
        const BRACKETED_PASTE = 1 << 8;
        /// Mode 1004 — Focus events.
        const FOCUS_EVENTS = 1 << 9;
        /// Mode 1049 — Alternate screen buffer (Feature 9).
        const ALTERNATE_SCREEN = 1 << 10;
        /// Mode 2026 — Synchronized output.
        const SYNCHRONIZED_OUTPUT = 1 << 11;
    }
}

impl ModeFlags {
    /// Return the mode flags that a terminal should have enabled by default.
    ///
    /// Specifically: [`ModeFlags::AUTO_WRAP`] and [`ModeFlags::CURSOR_VISIBLE`]
    /// are enabled.  All other flags start disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_vte::modes::ModeFlags;
    ///
    /// let m = ModeFlags::default_modes();
    /// assert!(m.contains(ModeFlags::AUTO_WRAP));
    /// assert!(m.contains(ModeFlags::CURSOR_VISIBLE));
    /// ```
    #[must_use]
    pub fn default_modes() -> Self {
        Self::AUTO_WRAP | Self::CURSOR_VISIBLE
    }
}
