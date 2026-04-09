/// VTE parser states based on the Paul Flo Williams state diagram.
///
/// <https://vt100.net/emu/dec_ansi_parser>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    Ground = 0,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsPassthrough,
    DcsIgnore,
    OscString,
    SosPmApcString,
}

use super::action::Action;

/// Compute the state transition for a given (state, byte) pair.
///
/// Returns `(action, new_state)`. When `new_state` is the same as the current
/// state, no state transition occurs. The "anywhere" rules (ESC, CAN, SUB) are
/// checked first to avoid duplicating them in every state.
///
/// This function is `#[inline]` because it is on the hot path — called once per
/// input byte.
#[must_use]
#[inline]
#[allow(clippy::too_many_lines)] // table-driven state machine — intentionally exhaustive
pub fn transition(state: State, byte: u8) -> (Action, State) {
    // ── Anywhere transitions (§ "anywhere" in the Williams diagram) ──────────
    match byte {
        // CAN / SUB cancel the current sequence and return to Ground.
        0x18 | 0x1A => return (Action::Execute, State::Ground),
        // ESC always begins a new escape sequence.
        0x1B => return (Action::None, State::Escape),
        // DEL is ignored in every state.
        0x7F => return (Action::Ignore, state),
        _ => {}
    }

    match state {
        // ── Ground ──────────────────────────────────────────────────────────
        //
        // 0x18, 0x1A, 0x1B, 0x7F are handled by the "anywhere" early-return
        // block above and never reach this arm; the wildcard arm is a dead
        // branch kept only for exhaustiveness.
        State::Ground => match byte {
            // C0 controls (excluding 0x18, 0x19, 0x1A, 0x1B which are handled
            // above, and 0x1C–0x1F which pass through to execute).
            0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x80..=0x8F | 0x91..=0x97 | 0x99 | 0x9A | 0x9C => {
                (Action::Execute, State::Ground)
            }
            // Printable ASCII and high UTF-8 bytes.
            0x20..=0x7E | 0xA0..=0xFF => (Action::Print, State::Ground),
            // C1 DCS (0x90) — single-byte form identical to ESC P.
            0x90 => (Action::None, State::DcsEntry),
            // C1 CSI (0x9B) — single-byte form identical to ESC [.
            0x9B => (Action::None, State::CsiEntry),
            // C1 OSC (0x9D) — single-byte form identical to ESC ].
            0x9D => (Action::OscStart, State::OscString),
            // SOS (0x98), PM (0x9E), APC (0x9F).
            0x98 | 0x9E | 0x9F => (Action::None, State::SosPmApcString),
            // Dead branch: bytes caught by "anywhere" block.
            _ => (Action::Ignore, State::Ground),
        },

        // ── Escape ──────────────────────────────────────────────────────────
        State::Escape => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Execute, State::Escape),
            // Intermediates: collect them, stay in EscapeIntermediate.
            0x20..=0x2F => (Action::Collect, State::EscapeIntermediate),
            // Final bytes: dispatch ESC and return to Ground.
            0x30..=0x4F | 0x51..=0x57 | 0x59 | 0x5A | 0x5C | 0x60..=0x7E => {
                (Action::EscDispatch, State::Ground)
            }
            // Sub-sequences entered via ESC + single character:
            0x50 => (Action::None, State::DcsEntry), // ESC P → DCS
            0x5B => (Action::None, State::CsiEntry), // ESC [ → CSI
            0x5D => (Action::OscStart, State::OscString), // ESC ] → OSC
            0x58 | 0x5E | 0x5F => (Action::None, State::SosPmApcString), // SOS/PM/APC
            _ => (Action::Ignore, State::Ground),
        },

        // ── EscapeIntermediate ───────────────────────────────────────────────
        State::EscapeIntermediate => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Execute, State::EscapeIntermediate),
            0x20..=0x2F => (Action::Collect, State::EscapeIntermediate),
            0x30..=0x7E => (Action::EscDispatch, State::Ground),
            _ => (Action::Ignore, State::Ground),
        },

        // ── CsiEntry ─────────────────────────────────────────────────────────
        State::CsiEntry => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Execute, State::CsiEntry),
            0x20..=0x2F => (Action::Collect, State::CsiIntermediate),
            0x30..=0x39 | 0x3B => (Action::Param, State::CsiParam),
            // 0x3C–0x3F are private-use parameter introducers (e.g., '?', '>').
            0x3C..=0x3F => (Action::Collect, State::CsiParam),
            0x40..=0x7E => (Action::CsiDispatch, State::Ground),
            // 0x3A and anything else → ignore the whole sequence.
            _ => (Action::Ignore, State::CsiIgnore),
        },

        // ── CsiParam ─────────────────────────────────────────────────────────
        State::CsiParam => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Execute, State::CsiParam),
            0x20..=0x2F => (Action::Collect, State::CsiIntermediate),
            0x30..=0x39 | 0x3B => (Action::Param, State::CsiParam),
            0x40..=0x7E => (Action::CsiDispatch, State::Ground),
            // 0x3A, 0x3C–0x3F, and anything else → ignore.
            _ => (Action::Ignore, State::CsiIgnore),
        },

        // ── CsiIntermediate ──────────────────────────────────────────────────
        State::CsiIntermediate => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Execute, State::CsiIntermediate),
            0x20..=0x2F => (Action::Collect, State::CsiIntermediate),
            0x40..=0x7E => (Action::CsiDispatch, State::Ground),
            // 0x30–0x3F and anything else → ignore the sequence.
            _ => (Action::Ignore, State::CsiIgnore),
        },

        // ── CsiIgnore ────────────────────────────────────────────────────────
        State::CsiIgnore => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Execute, State::CsiIgnore),
            0x40..=0x7E => (Action::Ignore, State::Ground),
            // 0x20–0x3F and anything else — stay in CsiIgnore.
            _ => (Action::Ignore, State::CsiIgnore),
        },

        // ── DcsEntry ─────────────────────────────────────────────────────────
        State::DcsEntry => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Ignore, State::DcsEntry),
            0x20..=0x2F => (Action::Collect, State::DcsIntermediate),
            0x30..=0x39 | 0x3B => (Action::Param, State::DcsParam),
            0x3C..=0x3F => (Action::Collect, State::DcsParam),
            0x40..=0x7E => (Action::Hook, State::DcsPassthrough),
            // 0x3A and anything else → ignore the whole DCS sequence.
            _ => (Action::Ignore, State::DcsIgnore),
        },

        // ── DcsParam ─────────────────────────────────────────────────────────
        State::DcsParam => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Ignore, State::DcsParam),
            0x20..=0x2F => (Action::Collect, State::DcsIntermediate),
            0x30..=0x39 | 0x3B => (Action::Param, State::DcsParam),
            0x40..=0x7E => (Action::Hook, State::DcsPassthrough),
            // 0x3A, 0x3C–0x3F, and anything else → ignore.
            _ => (Action::Ignore, State::DcsIgnore),
        },

        // ── DcsIntermediate ──────────────────────────────────────────────────
        State::DcsIntermediate => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F => (Action::Ignore, State::DcsIntermediate),
            0x20..=0x2F => (Action::Collect, State::DcsIntermediate),
            0x40..=0x7E => (Action::Hook, State::DcsPassthrough),
            // 0x30–0x3F and anything else → ignore the DCS sequence.
            _ => (Action::Ignore, State::DcsIgnore),
        },

        // ── DcsPassthrough ───────────────────────────────────────────────────
        State::DcsPassthrough => match byte {
            0x00..=0x17 | 0x19 | 0x1C..=0x1F | 0x20..=0x7E => (Action::Put, State::DcsPassthrough),
            0x9C => (Action::Unhook, State::Ground),
            _ => (Action::Ignore, State::DcsPassthrough),
        },

        // ── DcsIgnore ────────────────────────────────────────────────────────
        State::DcsIgnore => match byte {
            0x9C => (Action::Ignore, State::Ground),
            _ => (Action::Ignore, State::DcsIgnore),
        },

        // ── OscString ────────────────────────────────────────────────────────
        //
        // 0x1B is handled by the "anywhere" block above and transitions to
        // Escape; from there, b'\\' triggers EscDispatch while `prev_state` is
        // OscString — the Parser uses that to detect the ST terminator.
        State::OscString => match byte {
            // BEL or C1 ST (0x9C) terminate the OSC string.
            0x07 | 0x9C => (Action::OscEnd, State::Ground),
            // Printable ASCII and high bytes are accumulated.
            0x20..=0x7E | 0xA0..=0xFF => (Action::OscPut, State::OscString),
            // Everything else (control bytes, dead branches) is ignored.
            _ => (Action::Ignore, State::OscString),
        },

        // ── SosPmApcString ───────────────────────────────────────────────────
        State::SosPmApcString => match byte {
            0x9C => (Action::Ignore, State::Ground),
            _ => (Action::Ignore, State::SosPmApcString),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_ground_printable_gives_print() {
        let (action, next) = transition(State::Ground, b'A');
        assert_eq!(action, Action::Print);
        assert_eq!(next, State::Ground);
    }

    #[test]
    fn test_table_ground_newline_gives_execute() {
        let (action, next) = transition(State::Ground, b'\n');
        assert_eq!(action, Action::Execute);
        assert_eq!(next, State::Ground);
    }

    #[test]
    fn test_table_escape_starts_escape_state() {
        let (action, next) = transition(State::Ground, 0x1B);
        assert_eq!(action, Action::None);
        assert_eq!(next, State::Escape);
    }

    #[test]
    fn test_table_csi_entry_from_escape_bracket() {
        let (action, next) = transition(State::Escape, b'[');
        assert_eq!(action, Action::None);
        assert_eq!(next, State::CsiEntry);
    }

    #[test]
    fn test_table_can_returns_to_ground() {
        let (action, next) = transition(State::CsiParam, 0x18);
        assert_eq!(action, Action::Execute);
        assert_eq!(next, State::Ground);
    }

    #[test]
    fn test_table_sub_returns_to_ground() {
        let (action, next) = transition(State::EscapeIntermediate, 0x1A);
        assert_eq!(action, Action::Execute);
        assert_eq!(next, State::Ground);
    }

    #[test]
    fn test_table_del_ignored_in_all_states() {
        for state in
            [State::Ground, State::Escape, State::CsiEntry, State::CsiParam, State::OscString]
        {
            let (action, next_state) = transition(state, 0x7F);
            assert_eq!(action, Action::Ignore, "DEL must be ignored in {state:?}");
            assert_eq!(next_state, state, "DEL must not change state in {state:?}");
        }
    }
}
