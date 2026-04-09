#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! teamucks-core: Core multiplexer logic.
//!
//! Sessions, windows, panes, layout engine, server daemon, client protocol.

pub mod pty;
pub mod signal;
