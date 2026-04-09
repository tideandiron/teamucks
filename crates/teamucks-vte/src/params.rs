/// Maximum number of CSI/DCS parameters stored.
///
/// The VTE spec does not mandate a maximum, but real terminals cap at 16.
/// Parameters beyond this limit are silently dropped.
pub const MAX_PARAMS: usize = 16;

/// CSI / DCS parameter accumulator.
///
/// Collects decimal digit bytes and semicolon separators, building an array of
/// `u16` values suitable for passing to a [`crate::parser::Performer`].
///
/// # Overflow handling
///
/// Individual parameter values saturate at [`u16::MAX`] (65535) rather than
/// wrapping or panicking.
///
/// # Examples
///
/// ```
/// use teamucks_vte::params::Params;
///
/// let mut p = Params::new();
/// for byte in b"10;20;30" {
///     if *byte == b';' {
///         p.finish_param();
///     } else {
///         p.add_digit(*byte);
///     }
/// }
/// p.finalize();
/// assert_eq!(p.as_slice(), &[10, 20, 30]);
/// ```
#[derive(Debug, Clone)]
pub struct Params {
    /// Storage for completed parameter values.
    values: [u16; MAX_PARAMS],
    /// Number of completed values stored.
    count: usize,
    /// Accumulator for the parameter currently being parsed.
    current: u32,
    /// Whether any digit or separator has been seen since the last reset.
    has_current: bool,
}

impl Params {
    /// Create a new, empty parameter accumulator.
    #[must_use]
    pub const fn new() -> Self {
        Self { values: [0u16; MAX_PARAMS], count: 0, current: 0, has_current: false }
    }

    /// Reset the accumulator, discarding all stored parameters.
    #[inline]
    pub fn clear(&mut self) {
        self.count = 0;
        self.current = 0;
        self.has_current = false;
    }

    /// Accumulate a decimal digit byte (`b'0'`–`b'9'`).
    ///
    /// Values saturate at [`u16::MAX`] rather than wrapping.
    ///
    /// # Panics
    ///
    /// Does not panic. Non-digit bytes are silently ignored.
    #[inline]
    pub fn add_digit(&mut self, byte: u8) {
        let digit = u32::from(byte.wrapping_sub(b'0'));
        if digit > 9 {
            return;
        }
        self.has_current = true;
        // Saturating multiply-add: (current * 10 + digit).min(u16::MAX as u32)
        self.current =
            self.current.saturating_mul(10).saturating_add(digit).min(u32::from(u16::MAX));
    }

    /// Finish the current parameter (on encountering a semicolon separator).
    ///
    /// Pushes the accumulated value into the output array and resets the
    /// accumulator. If the array is already at capacity the value is dropped.
    #[inline]
    pub fn finish_param(&mut self) {
        // A semicolon with no preceding digits produces a 0 (default param).
        let value = u16::try_from(self.current).unwrap_or(u16::MAX);
        if self.count < MAX_PARAMS {
            self.values[self.count] = value;
            self.count += 1;
        }
        self.current = 0;
        self.has_current = true; // mark that a separator was seen
    }

    /// Finalize the accumulator before dispatching.
    ///
    /// If a parameter is in progress (digits seen or a trailing semicolon was
    /// present) it is pushed. After `finalize`, [`as_slice`] returns all
    /// parameters.
    ///
    /// [`as_slice`]: Params::as_slice
    #[inline]
    pub fn finalize(&mut self) {
        if self.has_current && self.count < MAX_PARAMS {
            let value = u16::try_from(self.current).unwrap_or(u16::MAX);
            self.values[self.count] = value;
            self.count += 1;
        }
        // No cleanup needed — `advance` will call `clear` before the next
        // sequence.
    }

    /// Return the slice of completed parameter values.
    ///
    /// Returns an empty slice if no parameters were parsed.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[u16] {
        &self.values[..self.count]
    }
}

impl Default for Params {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_single_value() {
        let mut p = Params::new();
        p.add_digit(b'5');
        p.finalize();
        assert_eq!(p.as_slice(), &[5]);
    }

    #[test]
    fn test_params_multiple_values() {
        let mut p = Params::new();
        for byte in b"10;20;30" {
            if *byte == b';' {
                p.finish_param();
            } else {
                p.add_digit(*byte);
            }
        }
        p.finalize();
        assert_eq!(p.as_slice(), &[10, 20, 30]);
    }

    #[test]
    fn test_params_empty_gives_empty_slice() {
        let mut p = Params::new();
        p.finalize();
        assert_eq!(p.as_slice(), &[]);
    }

    #[test]
    fn test_params_overflow_saturates_at_u16_max() {
        let mut p = Params::new();
        for byte in b"99999" {
            p.add_digit(*byte);
        }
        p.finalize();
        assert_eq!(p.as_slice(), &[u16::MAX]);
    }

    #[test]
    fn test_params_too_many_params_keeps_first_16() {
        let mut p = Params::new();
        // 18 semicolons → 19 parameters
        for i in 0u8..18 {
            p.add_digit(b'0' + (i % 10));
            p.finish_param();
        }
        p.add_digit(b'9');
        p.finalize();
        // Only first MAX_PARAMS are kept
        assert_eq!(p.as_slice().len(), MAX_PARAMS);
        assert_eq!(p.as_slice()[0], 0);
    }

    #[test]
    fn test_params_trailing_semicolon_produces_zero() {
        let mut p = Params::new();
        p.add_digit(b'5');
        p.finish_param(); // semicolon → pushes 5
        p.finalize(); // trailing → pushes 0
        assert_eq!(p.as_slice(), &[5, 0]);
    }

    #[test]
    fn test_params_clear_resets() {
        let mut p = Params::new();
        p.add_digit(b'9');
        p.finalize();
        assert_eq!(p.as_slice(), &[9]);
        p.clear();
        assert_eq!(p.as_slice(), &[]);
    }
}
