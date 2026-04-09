/// Session entity: owns an ordered list of windows.
///
/// A [`Session`] is the top tier of the teamucks hierarchy visible to users:
///
/// ```text
/// Server → Sessions → Windows → LayoutTree → Panes
/// ```
///
/// It tracks the currently active window by index, provides cyclic navigation,
/// and exposes cascade helpers so the server layer can respond to window
/// lifecycle events.
///
/// # Examples
///
/// ```
/// use teamucks_core::session::{Session, SessionId, SessionAction};
/// use teamucks_core::window::{Window, WindowId};
/// use teamucks_core::pane::PaneId;
///
/// let initial_window = Window::new_empty(WindowId(1), "main", PaneId(1));
/// let mut s = Session::new(SessionId(1), "default", initial_window);
/// assert_eq!(s.window_count(), 1);
/// ```
use std::time::Instant;

use crate::window::{Window, WindowId};

// ---------------------------------------------------------------------------
// SessionId
// ---------------------------------------------------------------------------

/// A unique identifier for a session.
///
/// # Examples
///
/// ```
/// use teamucks_core::session::SessionId;
/// let id = SessionId(1);
/// assert_eq!(id, SessionId(1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u32);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// SessionAction
// ---------------------------------------------------------------------------

/// The result of removing a window from a session.
///
/// The caller (server layer) inspects this to decide whether the session
/// should be destroyed.
///
/// # Examples
///
/// ```
/// use teamucks_core::session::{Session, SessionId, SessionAction};
/// use teamucks_core::window::{Window, WindowId};
/// use teamucks_core::pane::PaneId;
///
/// let initial = Window::new_empty(WindowId(1), "main", PaneId(1));
/// let mut s = Session::new(SessionId(1), "default", initial);
/// let action = s.remove_window(WindowId(1)).unwrap();
/// assert!(matches!(action, SessionAction::SessionEmpty));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    /// A window was removed; at least one window remains.
    WindowRemoved,
    /// The last window was removed; the session is now empty and should be
    /// destroyed.
    SessionEmpty,
}

// ---------------------------------------------------------------------------
// SessionError
// ---------------------------------------------------------------------------

/// Errors produced by [`Session`] operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The specified window does not exist in this session.
    #[error("window {0} not found in session")]
    WindowNotFound(WindowId),

    /// The window index is out of bounds.
    #[error("window index {0} is out of bounds")]
    IndexOutOfBounds(usize),
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A multiplexer session: an ordered list of [`Window`]s with an active index.
pub struct Session {
    id: SessionId,
    name: String,
    windows: Vec<Window>,
    active_window_index: usize,
    created_at: Instant,
}

impl Session {
    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// Create a new session with a single initial window.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let s = Session::new(SessionId(1), "default", w);
    /// assert_eq!(s.window_count(), 1);
    /// ```
    #[must_use]
    pub fn new(id: SessionId, name: &str, initial_window: Window) -> Self {
        Self {
            id,
            name: name.to_owned(),
            windows: vec![initial_window],
            active_window_index: 0,
            created_at: Instant::now(),
        }
    }

    // -----------------------------------------------------------------------
    // Window lifecycle
    // -----------------------------------------------------------------------

    /// Append `window` to the session's window list.
    ///
    /// The new window does **not** become active; the current active index is
    /// preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w1 = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let mut s = Session::new(SessionId(1), "default", w1);
    /// let w2 = Window::new_empty(WindowId(2), "extra", PaneId(2));
    /// s.add_window(w2);
    /// assert_eq!(s.window_count(), 2);
    /// ```
    pub fn add_window(&mut self, window: Window) {
        self.windows.push(window);
    }

    /// Remove the window with `window_id`.
    ///
    /// Returns [`SessionAction::SessionEmpty`] if the removed window was the
    /// last one.
    ///
    /// After removal the active index is clamped so it is always valid.  If
    /// the active window was removed the session switches to the window at the
    /// same index (now the former next window) or the last window if the
    /// removed window was at the end.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::WindowNotFound`] if no window with `window_id`
    /// exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId, SessionAction};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w1 = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let mut s = Session::new(SessionId(1), "default", w1);
    /// let w2 = Window::new_empty(WindowId(2), "extra", PaneId(2));
    /// s.add_window(w2);
    /// let action = s.remove_window(WindowId(2)).unwrap();
    /// assert!(matches!(action, SessionAction::WindowRemoved));
    /// ```
    pub fn remove_window(&mut self, window_id: WindowId) -> Result<SessionAction, SessionError> {
        let pos = self
            .windows
            .iter()
            .position(|w| w.id() == window_id)
            .ok_or(SessionError::WindowNotFound(window_id))?;

        self.windows.remove(pos);

        if self.windows.is_empty() {
            return Ok(SessionAction::SessionEmpty);
        }

        // Clamp active index so it remains valid.
        if self.active_window_index >= self.windows.len() {
            self.active_window_index = self.windows.len() - 1;
        } else if pos < self.active_window_index {
            // An earlier window was removed; shift the index down.
            self.active_window_index -= 1;
        }
        // If pos == active_window_index, the same index now points to the
        // former next window (or the last window if pos was at the end, which
        // was already clamped above).

        Ok(SessionAction::WindowRemoved)
    }

    /// Handle the cascade event where a window has become empty (its last pane
    /// closed).
    ///
    /// Removes the window from this session and returns the appropriate
    /// [`SessionAction`].
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId, SessionAction};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w1 = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let mut s = Session::new(SessionId(1), "s", w1);
    /// let action = s.handle_window_empty(WindowId(1));
    /// assert!(matches!(action, SessionAction::SessionEmpty));
    /// ```
    pub fn handle_window_empty(&mut self, window_id: WindowId) -> SessionAction {
        // Ignore the error — if the window is not found it was already removed.
        self.remove_window(window_id).unwrap_or(SessionAction::WindowRemoved)
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    /// Switch the active window to the one at `index`.
    ///
    /// Indices are zero-based.  If `index` is out of bounds the call is a
    /// no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w1 = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let mut s = Session::new(SessionId(1), "s", w1);
    /// let w2 = Window::new_empty(WindowId(2), "extra", PaneId(2));
    /// s.add_window(w2);
    /// s.switch_window(1);
    /// assert_eq!(s.active_window().id(), WindowId(2));
    /// ```
    pub fn switch_window(&mut self, index: usize) {
        if index < self.windows.len() {
            self.active_window_index = index;
        }
    }

    /// Move to the next window, wrapping around at the end.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w1 = Window::new_empty(WindowId(1), "a", PaneId(1));
    /// let mut s = Session::new(SessionId(1), "s", w1);
    /// let w2 = Window::new_empty(WindowId(2), "b", PaneId(2));
    /// s.add_window(w2);
    /// s.next_window();
    /// assert_eq!(s.active_window().id(), WindowId(2));
    /// s.next_window(); // wrap
    /// assert_eq!(s.active_window().id(), WindowId(1));
    /// ```
    pub fn next_window(&mut self) {
        if !self.windows.is_empty() {
            self.active_window_index = (self.active_window_index + 1) % self.windows.len();
        }
    }

    /// Move to the previous window, wrapping around at the start.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::session::{Session, SessionId};
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w1 = Window::new_empty(WindowId(1), "a", PaneId(1));
    /// let mut s = Session::new(SessionId(1), "s", w1);
    /// let w2 = Window::new_empty(WindowId(2), "b", PaneId(2));
    /// s.add_window(w2);
    /// s.prev_window();
    /// assert_eq!(s.active_window().id(), WindowId(2));
    /// ```
    pub fn prev_window(&mut self) {
        if !self.windows.is_empty() {
            if self.active_window_index == 0 {
                self.active_window_index = self.windows.len() - 1;
            } else {
                self.active_window_index -= 1;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Return this session's unique identifier.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Return the session name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the session name.
    pub fn set_name(&mut self, name: &str) {
        name.clone_into(&mut self.name);
    }

    /// Return the number of windows in this session.
    #[must_use]
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Return an immutable reference to the active window.
    ///
    /// # Panics
    ///
    /// Panics if `windows` is empty (invariant: a session always has at least
    /// one window until `remove_window` returns `SessionEmpty`).
    #[must_use]
    pub fn active_window(&self) -> &Window {
        // INVARIANT: active_window_index < windows.len() is maintained by all
        // mutating methods.  The slice index is therefore always valid.
        &self.windows[self.active_window_index]
    }

    /// Return a mutable reference to the active window.
    ///
    /// # Panics
    ///
    /// Panics if `windows` is empty (same invariant as [`active_window`]).
    ///
    /// [`active_window`]: Session::active_window
    pub fn active_window_mut(&mut self) -> &mut Window {
        &mut self.windows[self.active_window_index]
    }

    /// Return an immutable reference to the window with `window_id`, or `None`
    /// if it is not in this session.
    #[must_use]
    pub fn window(&self, window_id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id() == window_id)
    }

    /// Return a mutable reference to the window with `window_id`, or `None`
    /// if it is not in this session.
    pub fn window_mut(&mut self, window_id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id() == window_id)
    }

    /// Return a slice over all windows (used for status bar rendering).
    #[must_use]
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Return the instant at which this session was created.
    #[must_use]
    pub fn created_at(&self) -> Instant {
        self.created_at
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pane::PaneId,
        window::{Window, WindowId},
    };

    fn win(id: u32) -> Window {
        Window::new_empty(WindowId(id), &format!("win-{id}"), PaneId(id * 100))
    }

    fn session() -> Session {
        Session::new(SessionId(1), "test", win(1))
    }

    #[test]
    fn test_session_new_has_one_window() {
        let s = session();
        assert_eq!(s.window_count(), 1);
        assert_eq!(s.active_window().id(), WindowId(1));
    }

    #[test]
    fn test_session_add_window_increases_count() {
        let mut s = session();
        s.add_window(win(2));
        assert_eq!(s.window_count(), 2);
    }

    #[test]
    fn test_session_remove_non_last_window() {
        let mut s = session();
        s.add_window(win(2));
        let action = s.remove_window(WindowId(2)).unwrap();
        assert!(matches!(action, SessionAction::WindowRemoved));
        assert_eq!(s.window_count(), 1);
    }

    #[test]
    fn test_session_remove_last_window_empty() {
        let mut s = session();
        let action = s.remove_window(WindowId(1)).unwrap();
        assert!(matches!(action, SessionAction::SessionEmpty));
    }

    #[test]
    fn test_session_remove_nonexistent_window_errors() {
        let mut s = session();
        let err = s.remove_window(WindowId(99)).unwrap_err();
        assert!(matches!(err, SessionError::WindowNotFound(WindowId(99))));
    }

    #[test]
    fn test_session_switch_window_by_index() {
        let mut s = session();
        s.add_window(win(2));
        s.switch_window(1);
        assert_eq!(s.active_window().id(), WindowId(2));
    }

    #[test]
    fn test_session_switch_out_of_bounds_noop() {
        let mut s = session();
        s.switch_window(99); // out of bounds, should be no-op
        assert_eq!(s.active_window().id(), WindowId(1));
    }

    #[test]
    fn test_session_next_window_wraps() {
        let mut s = session();
        s.add_window(win(2));
        s.next_window();
        assert_eq!(s.active_window().id(), WindowId(2));
        s.next_window(); // wrap
        assert_eq!(s.active_window().id(), WindowId(1));
    }

    #[test]
    fn test_session_prev_window_wraps() {
        let mut s = session();
        s.add_window(win(2));
        s.prev_window(); // wraps to last
        assert_eq!(s.active_window().id(), WindowId(2));
        s.prev_window();
        assert_eq!(s.active_window().id(), WindowId(1));
    }

    #[test]
    fn test_session_set_name() {
        let mut s = session();
        s.set_name("renamed");
        assert_eq!(s.name(), "renamed");
    }

    #[test]
    fn test_session_window_by_id_found() {
        let mut s = session();
        s.add_window(win(2));
        assert!(s.window(WindowId(1)).is_some());
        assert!(s.window(WindowId(2)).is_some());
    }

    #[test]
    fn test_session_window_by_id_not_found() {
        let s = session();
        assert!(s.window(WindowId(99)).is_none());
    }

    #[test]
    fn test_session_active_index_clamped_after_remove() {
        let mut s = session();
        s.add_window(win(2));
        s.add_window(win(3));
        s.switch_window(2); // active = index 2 (win 3)
        s.remove_window(WindowId(3)).unwrap(); // removes index 2
                                               // Active index should now be clamped to 1.
        assert_eq!(s.window_count(), 2);
        assert!(s.active_window_index < s.windows.len());
    }

    #[test]
    fn test_session_handle_window_empty() {
        let mut s = session();
        s.add_window(win(2));
        let action = s.handle_window_empty(WindowId(2));
        assert!(matches!(action, SessionAction::WindowRemoved));
        assert_eq!(s.window_count(), 1);
    }
}
