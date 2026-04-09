//! Input handling for the teamucks multiplexer.
//!
//! This module provides the input pipeline that sits between raw terminal key
//! events and the multiplexer's session/window/pane model.
//!
//! # Architecture
//!
//! ```text
//! Terminal (PTY) ──► KeyEvent ──► InputStateMachine ──► InputAction
//!                                                             │
//!                                          ┌──────────────────┤
//!                                          │                  │
//!                                    ForwardToPane    ExecuteCommand
//!                                          │                  │
//!                                     Active pane     Command dispatcher
//! ```
//!
//! ## Key types
//!
//! - [`key::KeyEvent`] — a logical key + modifiers pair.
//! - [`command::Command`] — a high-level multiplexer command.
//! - [`prefix::InputStateMachine`] — the state machine that translates key
//!   sequences into commands.
//! - [`prefix::InputAction`] — the decision returned per keypress.

pub mod command;
pub mod key;
pub mod prefix;
