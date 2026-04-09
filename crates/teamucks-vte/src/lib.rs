#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! teamucks-vte: A correct, high-performance virtual terminal emulator library.
//!
//! This crate provides a VTE parser and terminal grid that interprets
//! byte streams from terminal applications and maintains a grid of styled cells.

#[cfg(test)]
mod tests {
    #[test]
    fn test_crate_compiles() {
        // Placeholder: verifies the crate links and the test harness works.
        // Real behavioral tests land in feat/vte-parser-core.
        let x: u8 = 1;
        assert_eq!(x, 1);
    }
}
