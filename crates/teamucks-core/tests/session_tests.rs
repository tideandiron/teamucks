/// Integration tests for the Window and Session model (Feature 20).
///
/// Tests follow strict TDD: each test was written before its corresponding
/// implementation.  Tests are named `test_<unit>_<scenario>` per the project
/// guidelines.
use teamucks_core::{
    layout::Direction,
    pane::PaneId,
    session::{Session, SessionAction, SessionId},
    window::{Window, WindowAction, WindowId},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Make a `Window` with a single pane that has no real PTY.
///
/// We use `Window::new_empty` which is a test-only constructor that does not
/// spawn a child process.  All pane operations involving PTY I/O are tested
/// in the dedicated `pane_tests.rs`; here we only test the lifecycle model.
fn make_window(id: u32, name: &str) -> Window {
    Window::new_empty(WindowId(id), name, PaneId(id * 100))
}

fn make_session(id: u32, name: &str) -> Session {
    let window = make_window(id, &format!("win-{id}"));
    Session::new(SessionId(id), name, window)
}

// ===========================================================================
// Window tests
// ===========================================================================

#[test]
fn test_window_create() {
    let w = make_window(1, "main");
    assert_eq!(w.id(), WindowId(1));
    assert_eq!(w.name(), "main");
    assert_eq!(w.pane_count(), 1);
    assert!(!w.has_activity());
}

#[test]
fn test_window_split() {
    let mut w = make_window(1, "main");
    let new_id = PaneId(200);
    w.split_active(Direction::Vertical, 0.5, new_id).expect("split must succeed");
    assert_eq!(w.pane_count(), 2);
}

#[test]
fn test_window_close_pane() {
    let mut w = make_window(1, "main");
    let new_id = PaneId(200);
    w.split_active(Direction::Vertical, 0.5, new_id).expect("split must succeed");
    assert_eq!(w.pane_count(), 2);
    let action = w.close_pane(new_id).expect("close must succeed");
    assert!(matches!(action, WindowAction::PaneClosed));
    assert_eq!(w.pane_count(), 1);
}

#[test]
fn test_window_close_last_pane() {
    let mut w = make_window(1, "main");
    let only_pane = PaneId(100);
    let action = w.close_pane(only_pane).expect("close must succeed");
    assert!(
        matches!(action, WindowAction::WindowEmpty),
        "closing last pane must return WindowEmpty"
    );
}

#[test]
fn test_window_navigate() {
    let mut w = make_window(1, "main");
    let first_pane = PaneId(100);
    let second_pane = PaneId(200);
    w.split_active(Direction::Vertical, 0.5, second_pane).expect("split must succeed");

    // After split the active pane is still the first.
    assert_eq!(w.active_pane_id(), first_pane);

    // Navigate to the right (Vertical = left/right axis).
    w.navigate(Direction::Vertical);
    assert_eq!(w.active_pane_id(), second_pane);
}

#[test]
fn test_window_rename() {
    let mut w = make_window(1, "original");
    w.set_name("renamed");
    assert_eq!(w.name(), "renamed");
}

#[test]
fn test_window_activity_flag() {
    let mut w = make_window(1, "main");
    assert!(!w.has_activity());
    w.set_activity(true);
    assert!(w.has_activity());
    w.set_activity(false);
    assert!(!w.has_activity());
}

#[test]
fn test_window_pane_count() {
    let mut w = make_window(1, "main");
    assert_eq!(w.pane_count(), 1);

    w.split_active(Direction::Vertical, 0.5, PaneId(200)).unwrap();
    assert_eq!(w.pane_count(), 2);

    w.split_active(Direction::Horizontal, 0.5, PaneId(300)).unwrap();
    assert_eq!(w.pane_count(), 3);

    // Close one pane.
    w.close_pane(PaneId(300)).unwrap();
    assert_eq!(w.pane_count(), 2);
}

#[test]
fn test_window_resize() {
    let mut w = make_window(1, "main");
    w.split_active(Direction::Vertical, 0.5, PaneId(200)).unwrap();
    // Resize should not panic and the layout should still reflect two panes.
    w.resize(160, 48);
    assert_eq!(w.pane_count(), 2);
}

#[test]
fn test_window_active_pane_changes_on_close() {
    let mut w = make_window(1, "main");
    let first = PaneId(100);
    let second = PaneId(200);
    w.split_active(Direction::Vertical, 0.5, second).unwrap();

    // Move focus to second.
    w.navigate(Direction::Vertical);
    assert_eq!(w.active_pane_id(), second);

    // Close the active pane — active must shift to a remaining pane.
    let action = w.close_pane(second).unwrap();
    assert!(matches!(action, WindowAction::PaneClosed));
    assert_eq!(w.active_pane_id(), first);
}

#[test]
fn test_window_layout_accessor() {
    let w = make_window(1, "main");
    // layout() must return a reference; it should contain the root pane.
    let _ = w.layout();
}

// ===========================================================================
// Session tests
// ===========================================================================

#[test]
fn test_session_create() {
    let s = make_session(1, "default");
    assert_eq!(s.id(), SessionId(1));
    assert_eq!(s.name(), "default");
    assert_eq!(s.window_count(), 1);
}

#[test]
fn test_session_add_window() {
    let mut s = make_session(1, "default");
    let w2 = make_window(2, "second");
    s.add_window(w2);
    assert_eq!(s.window_count(), 2);
}

#[test]
fn test_session_remove_window() {
    let mut s = make_session(1, "default");
    s.add_window(make_window(2, "second"));
    assert_eq!(s.window_count(), 2);
    let action = s.remove_window(WindowId(2)).expect("remove must succeed");
    assert!(matches!(action, SessionAction::WindowRemoved));
    assert_eq!(s.window_count(), 1);
}

#[test]
fn test_session_remove_last_window() {
    let mut s = make_session(1, "default");
    let action = s.remove_window(WindowId(1)).expect("remove must succeed");
    assert!(
        matches!(action, SessionAction::SessionEmpty),
        "removing last window must return SessionEmpty"
    );
}

#[test]
fn test_session_switch_window() {
    let mut s = make_session(1, "default");
    s.add_window(make_window(2, "second"));
    s.add_window(make_window(3, "third"));

    s.switch_window(2);
    assert_eq!(s.active_window().id(), WindowId(3));
}

#[test]
fn test_session_next_prev_window() {
    let mut s = make_session(1, "default");
    s.add_window(make_window(2, "second"));
    s.add_window(make_window(3, "third"));

    // Starts at index 0.
    s.next_window();
    assert_eq!(s.active_window().id(), WindowId(2));

    s.next_window();
    assert_eq!(s.active_window().id(), WindowId(3));

    // Wrap around.
    s.next_window();
    assert_eq!(s.active_window().id(), WindowId(1));

    s.prev_window();
    assert_eq!(s.active_window().id(), WindowId(3));
}

#[test]
fn test_session_rename() {
    let mut s = make_session(1, "original");
    s.set_name("renamed");
    assert_eq!(s.name(), "renamed");
}

#[test]
fn test_session_windows_slice() {
    let mut s = make_session(1, "default");
    s.add_window(make_window(2, "second"));
    let windows = s.windows();
    assert_eq!(windows.len(), 2);
}

#[test]
fn test_session_active_window_mut() {
    let mut s = make_session(1, "default");
    let w = s.active_window_mut();
    w.set_name("mutated");
    assert_eq!(s.active_window().name(), "mutated");
}

#[test]
fn test_session_window_by_id() {
    let mut s = make_session(1, "default");
    s.add_window(make_window(2, "second"));

    assert!(s.window(WindowId(1)).is_some());
    assert!(s.window(WindowId(2)).is_some());
    assert!(s.window(WindowId(99)).is_none());
}

// ===========================================================================
// Cascade tests
// ===========================================================================

#[test]
fn test_cascade_close_pane_to_window() {
    // Closing the last pane in a window should signal the session to remove
    // that window.  We simulate this by calling close_pane on the only pane
    // and checking that WindowEmpty is returned.
    let mut s = make_session(1, "default");
    s.add_window(make_window(2, "second"));

    // Close the only pane in window 2.
    let win2 = s.window_mut(WindowId(2)).unwrap();
    let action = win2.close_pane(PaneId(200)).unwrap();
    assert!(matches!(action, WindowAction::WindowEmpty));

    // Now the session layer should remove window 2.
    let session_action = s.handle_window_empty(WindowId(2));
    assert!(matches!(session_action, SessionAction::WindowRemoved));
    assert_eq!(s.window_count(), 1);
}

#[test]
fn test_cascade_close_window_to_session() {
    // When the last window is removed from a session, SessionEmpty is returned.
    let mut s = make_session(1, "default");

    // Only one window.  Remove it.
    let win = s.active_window_mut();
    let window_action = win.close_pane(PaneId(100)).unwrap();
    assert!(matches!(window_action, WindowAction::WindowEmpty));

    let session_action = s.handle_window_empty(WindowId(1));
    assert!(matches!(session_action, SessionAction::SessionEmpty));
}

// ===========================================================================
// Pane exit behavior tests
// ===========================================================================

#[test]
fn test_pane_exit_close() {
    use teamucks_core::pane::ExitBehavior;

    // ExitBehavior::Close is the default.  When a pane marks itself as exited
    // with this behavior the result signals that the pane should be closed.
    let behavior = ExitBehavior::Close;
    let exit_code = 0;
    let result = behavior.on_exit(exit_code);
    assert!(
        matches!(result, teamucks_core::pane::ExitAction::Close),
        "ExitBehavior::Close must produce ExitAction::Close"
    );
}

#[test]
fn test_pane_exit_hold() {
    use teamucks_core::pane::ExitBehavior;

    let behavior = ExitBehavior::Hold;
    let exit_code = 1;
    let result = behavior.on_exit(exit_code);
    assert!(
        matches!(result, teamucks_core::pane::ExitAction::Hold { code: 1 }),
        "ExitBehavior::Hold must produce ExitAction::Hold with the exit code"
    );
}
