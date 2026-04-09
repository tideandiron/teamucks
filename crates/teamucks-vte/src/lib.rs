#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! teamucks-vte: A correct, high-performance virtual terminal emulator library.
//!
//! This crate provides a VTE parser and terminal grid that interprets
//! byte streams from terminal applications and maintains a grid of styled cells.
//!
//! # Architecture
//!
//! - [`parser`] — Table-driven VTE state machine. Takes raw bytes, emits
//!   [`parser::Performer`] callbacks.
//! - [`params`] — CSI/DCS parameter accumulator used internally by the parser.

pub mod params;
pub mod parser;
