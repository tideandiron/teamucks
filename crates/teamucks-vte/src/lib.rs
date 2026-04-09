#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! teamucks-vte: A correct, high-performance VTE parser library.
//!
//! This crate provides a table-driven VTE state machine parser that interprets
//! byte streams from terminal applications and emits typed events to a
//! [`parser::Performer`] implementation.
//!
//! # Architecture
//!
//! - [`parser`] — Table-driven VTE state machine. Takes raw bytes, emits
//!   [`parser::Performer`] callbacks for printable characters, control
//!   sequences (CSI, ESC, DCS, OSC), and UTF-8 decoded text.
//! - [`params`] — CSI/DCS parameter accumulator used internally by the parser.

pub mod cell;
pub mod grid;
pub mod modes;
pub mod params;
pub mod parser;
pub mod row;
pub mod style;
pub mod terminal;

#[cfg(test)]
mod modes_tests;

#[cfg(test)]
mod alternate_screen_tests;
