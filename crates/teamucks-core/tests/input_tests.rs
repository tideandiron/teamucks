/// Integration tests for Feature 17: Input Handling & Prefix Key State Machine.
///
/// Tests are organized by concern:
/// 1. Key representation
/// 2. Passthrough behaviour
/// 3. Prefix key interaction
/// 4. Resize mode
/// 5. State-cycle regression tests
/// 6. Custom prefix key
use teamucks_core::input::{
    command::Command,
    key::{Key, KeyEvent, Modifiers},
    prefix::{InputAction, InputStateMachine},
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn ctrl_space() -> KeyEvent {
    KeyEvent { key: Key::Char(' '), modifiers: Modifiers::CTRL }
}

fn plain(c: char) -> KeyEvent {
    KeyEvent { key: Key::Char(c), modifiers: Modifiers::empty() }
}

fn special(key: Key) -> KeyEvent {
    KeyEvent { key, modifiers: Modifiers::empty() }
}

fn default_sm() -> InputStateMachine {
    InputStateMachine::new(ctrl_space(), std::time::Duration::from_secs(1))
}

// ── Key representation ────────────────────────────────────────────────────────

#[test]
fn test_key_event_equality() {
    let a = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
    let b = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
    assert_eq!(a, b);
}

#[test]
fn test_key_event_inequality_different_modifiers() {
    let a = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
    let b = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::ALT };
    assert_ne!(a, b);
}

#[test]
fn test_key_event_ctrl_space() {
    let key = ctrl_space();
    assert_eq!(key.key, Key::Char(' '));
    assert!(key.modifiers.contains(Modifiers::CTRL));
    assert!(!key.modifiers.contains(Modifiers::ALT));
}

// ── Passthrough ───────────────────────────────────────────────────────────────

#[test]
fn test_passthrough_normal_keys() {
    let mut sm = default_sm();
    for c in ['a', 'b', 'c', 'z', 'A', 'Z', '1', '!'] {
        let key = plain(c);
        let action = sm.process_key(&key);
        assert_eq!(
            action,
            InputAction::ForwardToPane(key.clone()),
            "expected {c:?} to be forwarded in passthrough"
        );
    }
}

#[test]
fn test_passthrough_special_keys() {
    let mut sm = default_sm();
    for key in [Key::Enter, Key::Escape, Key::Backspace, Key::Tab, Key::Up, Key::Down] {
        let event = special(key.clone());
        let action = sm.process_key(&event);
        assert_eq!(
            action,
            InputAction::ForwardToPane(event.clone()),
            "expected {key:?} to be forwarded in passthrough"
        );
    }
}

// ── Prefix key ────────────────────────────────────────────────────────────────

#[test]
fn test_prefix_key_enters_prefix_mode() {
    let mut sm = default_sm();
    let action = sm.process_key(&ctrl_space());
    assert_eq!(action, InputAction::Consumed, "prefix key must be consumed");
    assert!(sm.is_prefix_active(), "state must be PrefixActive after prefix key");
}

#[test]
fn test_prefix_plus_pipe_splits_vertical() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('|'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::SplitVertical));
}

#[test]
fn test_prefix_plus_dash_splits_horizontal() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('-'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::SplitHorizontal));
}

#[test]
fn test_prefix_plus_x_closes_pane() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('x'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::ClosePane));
}

#[test]
fn test_prefix_plus_z_zooms() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('z'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::ZoomPane));
}

#[test]
fn test_prefix_plus_hjkl_navigates() {
    let cases = [
        ('h', Command::NavigateLeft),
        ('j', Command::NavigateDown),
        ('k', Command::NavigateUp),
        ('l', Command::NavigateRight),
    ];
    for (c, expected) in cases {
        let mut sm = default_sm();
        sm.process_key(&ctrl_space());
        let action = sm.process_key(&plain(c));
        assert_eq!(
            action,
            InputAction::ExecuteCommand(expected.clone()),
            "prefix + {c:?} should produce {expected:?}"
        );
    }
}

#[test]
fn test_prefix_plus_d_detaches() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('d'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::Detach));
}

#[test]
fn test_prefix_plus_c_creates_window() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('c'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::CreateWindow));
}

#[test]
fn test_prefix_plus_n_next_window() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('n'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::NextWindow));
}

#[test]
fn test_prefix_plus_digit_goto() {
    for n in 0u8..=9 {
        let mut sm = default_sm();
        sm.process_key(&ctrl_space());
        let c = char::from_digit(u32::from(n), 10).expect("single decimal digit");
        let action = sm.process_key(&plain(c));
        assert_eq!(
            action,
            InputAction::ExecuteCommand(Command::GoToWindow(n)),
            "prefix + {n} should GoToWindow({n})"
        );
    }
}

#[test]
fn test_prefix_unknown_key_consumed() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    // 'Q' has no binding
    let action = sm.process_key(&plain('Q'));
    assert_eq!(action, InputAction::Consumed, "unknown key in prefix mode must be consumed");
    // Must now be back in passthrough
    assert!(!sm.is_prefix_active(), "must return to passthrough after unknown prefix key");
    // Confirm next key is forwarded (passthrough)
    let action2 = sm.process_key(&plain('a'));
    assert_eq!(action2, InputAction::ForwardToPane(plain('a')));
}

// ── Resize mode ───────────────────────────────────────────────────────────────

#[test]
fn test_prefix_r_enters_resize() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    let action = sm.process_key(&plain('r'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::EnterResizeMode));
    assert!(sm.is_resize_active(), "state must be ResizeMode after prefix+r");
}

#[test]
fn test_resize_hjkl() {
    let cases = [
        ('h', Command::ResizeLeft),
        ('j', Command::ResizeDown),
        ('k', Command::ResizeUp),
        ('l', Command::ResizeRight),
    ];
    for (c, expected) in cases {
        let mut sm = default_sm();
        sm.process_key(&ctrl_space());
        sm.process_key(&plain('r')); // enter resize mode
        let action = sm.process_key(&plain(c));
        assert_eq!(
            action,
            InputAction::ExecuteCommand(expected.clone()),
            "resize mode: {c:?} should produce {expected:?}"
        );
        // Resize mode persists after a resize command
        assert!(sm.is_resize_active(), "must remain in ResizeMode after resize command");
    }
}

#[test]
fn test_resize_equals_equalizes() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    sm.process_key(&plain('r'));
    let action = sm.process_key(&plain('='));
    assert_eq!(action, InputAction::ExecuteCommand(Command::EqualizeSplits));
    assert!(sm.is_resize_active(), "must remain in ResizeMode after equalize");
}

#[test]
fn test_resize_escape_exits() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    sm.process_key(&plain('r'));
    assert!(sm.is_resize_active());
    let action = sm.process_key(&special(Key::Escape));
    assert_eq!(action, InputAction::ExecuteCommand(Command::ExitMode));
    assert!(!sm.is_resize_active(), "must leave ResizeMode after Escape");
    // Confirm passthrough is restored
    let fwd = sm.process_key(&plain('a'));
    assert_eq!(fwd, InputAction::ForwardToPane(plain('a')));
}

#[test]
fn test_resize_unknown_consumed() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    sm.process_key(&plain('r'));
    let action = sm.process_key(&plain('Q'));
    assert_eq!(action, InputAction::Consumed, "unknown key in resize mode must be consumed");
    // Must remain in resize mode (not exit on unknown)
    assert!(sm.is_resize_active(), "must stay in ResizeMode after unknown key");
}

// ── Return to passthrough ─────────────────────────────────────────────────────

#[test]
fn test_command_returns_to_passthrough() {
    let mut sm = default_sm();
    sm.process_key(&ctrl_space());
    sm.process_key(&plain('|')); // SplitVertical — should return to passthrough
    assert!(!sm.is_prefix_active(), "must return to passthrough after executing a command");
    let action = sm.process_key(&plain('a'));
    assert_eq!(action, InputAction::ForwardToPane(plain('a')));
}

#[test]
fn test_prefix_state_reset() {
    // Full cycle: passthrough → prefix → command → passthrough → prefix → …
    let mut sm = default_sm();

    // Round 1
    assert_eq!(sm.process_key(&plain('a')), InputAction::ForwardToPane(plain('a')));
    assert_eq!(sm.process_key(&ctrl_space()), InputAction::Consumed);
    assert_eq!(sm.process_key(&plain('x')), InputAction::ExecuteCommand(Command::ClosePane));

    // Round 2
    assert_eq!(sm.process_key(&plain('b')), InputAction::ForwardToPane(plain('b')));
    assert_eq!(sm.process_key(&ctrl_space()), InputAction::Consumed);
    assert_eq!(sm.process_key(&plain('c')), InputAction::ExecuteCommand(Command::CreateWindow));
}

// ── Custom prefix key ─────────────────────────────────────────────────────────

#[test]
fn test_custom_prefix_key() {
    // Use Ctrl-A instead of Ctrl-Space
    let ctrl_a = KeyEvent { key: Key::Char('a'), modifiers: Modifiers::CTRL };
    let mut sm = InputStateMachine::new(ctrl_a.clone(), std::time::Duration::from_secs(1));

    // Ctrl-Space should now pass through
    let action = sm.process_key(&ctrl_space());
    assert_eq!(
        action,
        InputAction::ForwardToPane(ctrl_space()),
        "Ctrl-Space must be forwarded when Ctrl-A is the prefix"
    );

    // Ctrl-A is the prefix now
    let action = sm.process_key(&ctrl_a);
    assert_eq!(action, InputAction::Consumed);
    assert!(sm.is_prefix_active());

    // Confirm a known binding works
    let action = sm.process_key(&plain('d'));
    assert_eq!(action, InputAction::ExecuteCommand(Command::Detach));
}
