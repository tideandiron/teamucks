/// Window entity: owns a layout tree and a set of pane identifiers.
///
/// A [`Window`] is the second tier of the teamucks hierarchy:
///
/// ```text
/// Server → Sessions → Windows → LayoutTree → Panes
/// ```
///
/// Each window holds a [`LayoutTree`] that arranges its panes spatially and
/// tracks which pane is active.  The window does **not** own live [`Pane`]
/// values — live panes are owned by the session actor task and accessed by
/// [`PaneId`].  In tests a lightweight "empty" constructor is provided that
/// requires no PTY.
///
/// # Examples
///
/// ```
/// use teamucks_core::window::{Window, WindowId, WindowAction};
/// use teamucks_core::layout::Direction;
/// use teamucks_core::pane::PaneId;
///
/// let mut w = Window::new_empty(WindowId(1), "main", PaneId(1));
/// assert_eq!(w.pane_count(), 1);
/// w.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
/// assert_eq!(w.pane_count(), 2);
/// ```
use crate::{
    layout::{resolve, Direction, LayoutError, LayoutTree},
    pane::PaneId,
};

// ---------------------------------------------------------------------------
// WindowId
// ---------------------------------------------------------------------------

/// A unique identifier for a window within a session.
///
/// # Examples
///
/// ```
/// use teamucks_core::window::WindowId;
/// let id = WindowId(1);
/// assert_eq!(id, WindowId(1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u32);

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "window:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// WindowAction
// ---------------------------------------------------------------------------

/// The result of closing a pane in a window.
///
/// The caller (session layer) inspects this to decide whether the window
/// should be removed from the session.
///
/// # Examples
///
/// ```
/// use teamucks_core::window::{Window, WindowId, WindowAction};
/// use teamucks_core::pane::PaneId;
///
/// let mut w = Window::new_empty(WindowId(1), "main", PaneId(1));
/// let action = w.close_pane(PaneId(1)).unwrap();
/// assert!(matches!(action, WindowAction::WindowEmpty));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    /// A pane was closed; at least one pane remains.
    PaneClosed,
    /// The last pane was closed; the window is now empty and should be removed.
    WindowEmpty,
}

// ---------------------------------------------------------------------------
// WindowError
// ---------------------------------------------------------------------------

/// Errors produced by [`Window`] operations.
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    /// Underlying layout engine failure.
    #[error("layout error: {0}")]
    Layout(#[from] LayoutError),

    /// The specified pane does not exist in this window.
    #[error("pane {0} not found in window")]
    PaneNotFound(PaneId),
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

/// A window containing a layout tree of pane identifiers.
///
/// The window tracks which pane is active and forwards lifecycle operations to
/// the [`LayoutTree`].
pub struct Window {
    id: WindowId,
    name: String,
    layout: LayoutTree,
    /// Number of panes currently in the layout tree.
    pane_count: usize,
    has_activity: bool,
}

impl Window {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Create a new window with a single empty (no-PTY) pane.
    ///
    /// This constructor is suitable for tests and for the initial window
    /// created during session setup before panes are spawned.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// assert_eq!(w.name(), "main");
    /// assert_eq!(w.pane_count(), 1);
    /// ```
    #[must_use]
    pub fn new_empty(id: WindowId, name: &str, initial_pane: PaneId) -> Self {
        let layout = LayoutTree::new(initial_pane);
        Self { id, name: name.to_owned(), layout, pane_count: 1, has_activity: false }
    }

    /// Create a new window with explicit dimensions for split validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w = Window::new_with_dimensions(WindowId(1), "main", PaneId(1), 160, 48);
    /// assert_eq!(w.pane_count(), 1);
    /// ```
    #[must_use]
    pub fn new_with_dimensions(
        id: WindowId,
        name: &str,
        initial_pane: PaneId,
        cols: u16,
        rows: u16,
    ) -> Self {
        let layout = LayoutTree::with_dimensions(initial_pane, cols, rows);
        Self { id, name: name.to_owned(), layout, pane_count: 1, has_activity: false }
    }

    // -----------------------------------------------------------------------
    // Pane lifecycle
    // -----------------------------------------------------------------------

    /// Split the active pane in `direction`, inserting `new_pane_id` as the
    /// second child.
    ///
    /// `ratio` is the fraction of the parent's dimension allocated to the
    /// original (first) pane.
    ///
    /// # Errors
    ///
    /// Returns [`WindowError::Layout`] if the split would violate minimum pane
    /// dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::layout::Direction;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut w = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// w.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
    /// assert_eq!(w.pane_count(), 2);
    /// ```
    pub fn split_active(
        &mut self,
        direction: Direction,
        ratio: f32,
        new_pane_id: PaneId,
    ) -> Result<PaneId, WindowError> {
        let active = self.layout.active_pane;
        self.layout.split(active, direction, ratio, new_pane_id)?;
        self.pane_count += 1;
        Ok(new_pane_id)
    }

    /// Close `pane_id`, returning [`WindowAction::WindowEmpty`] if it was the
    /// last pane.
    ///
    /// # Errors
    ///
    /// Returns [`WindowError::PaneNotFound`] if `pane_id` is not in this
    /// window's layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId, WindowAction};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut w = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// let action = w.close_pane(PaneId(1)).unwrap();
    /// assert!(matches!(action, WindowAction::WindowEmpty));
    /// ```
    pub fn close_pane(&mut self, pane_id: PaneId) -> Result<WindowAction, WindowError> {
        if self.pane_count == 1 {
            // This is the last pane; verify it is indeed in our layout.
            let mut ids = Vec::new();
            self.layout.root.collect_ids(&mut ids);
            if ids.contains(&pane_id) {
                self.pane_count = 0;
                return Ok(WindowAction::WindowEmpty);
            }
            return Err(WindowError::PaneNotFound(pane_id));
        }

        self.layout.close(pane_id)?;
        self.pane_count -= 1;
        Ok(WindowAction::PaneClosed)
    }

    /// Navigate the active pane to the nearest neighbour in `direction`.
    ///
    /// Uses the layout engine's spatial navigation.  If no neighbour exists in
    /// the primary direction the call is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::layout::Direction;
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut w = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// w.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
    /// w.navigate(Direction::Vertical);
    /// assert_eq!(w.active_pane_id(), PaneId(2));
    /// ```
    pub fn navigate(&mut self, direction: Direction) {
        let geoms =
            resolve::resolve(&self.layout, self.layout.window_width, self.layout.window_height);
        if let Some(next) = crate::layout::navigate::navigate(
            &self.layout,
            self.layout.active_pane,
            direction,
            &geoms,
        ) {
            self.layout.active_pane = next;
        }
    }

    /// Resize the layout to the new `cols × rows`, updating split-validation
    /// hints.
    ///
    /// This does not touch live PTY processes; callers are responsible for
    /// sending `SIGWINCH` to each pane after calling this.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let mut w = Window::new_empty(WindowId(1), "main", PaneId(1));
    /// w.resize(160, 48);
    /// ```
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.layout.set_dimensions(cols, rows);
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Return this window's unique identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use teamucks_core::window::{Window, WindowId};
    /// use teamucks_core::pane::PaneId;
    ///
    /// let w = Window::new_empty(WindowId(42), "win", PaneId(1));
    /// assert_eq!(w.id(), WindowId(42));
    /// ```
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Return the window name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the window name.
    pub fn set_name(&mut self, name: &str) {
        name.clone_into(&mut self.name);
    }

    /// Return `true` if the activity flag is set.
    #[must_use]
    pub fn has_activity(&self) -> bool {
        self.has_activity
    }

    /// Set or clear the activity flag.
    pub fn set_activity(&mut self, active: bool) {
        self.has_activity = active;
    }

    /// Return the number of panes currently in this window.
    #[must_use]
    pub fn pane_count(&self) -> usize {
        self.pane_count
    }

    /// Return an immutable reference to the layout tree.
    #[must_use]
    pub fn layout(&self) -> &LayoutTree {
        &self.layout
    }

    /// Return the [`PaneId`] of the currently active pane.
    #[must_use]
    pub fn active_pane_id(&self) -> PaneId {
        self.layout.active_pane
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Direction;

    fn w() -> Window {
        Window::new_empty(WindowId(1), "test", PaneId(1))
    }

    #[test]
    fn test_window_initial_state() {
        let win = w();
        assert_eq!(win.id(), WindowId(1));
        assert_eq!(win.name(), "test");
        assert_eq!(win.pane_count(), 1);
        assert!(!win.has_activity());
        assert_eq!(win.active_pane_id(), PaneId(1));
    }

    #[test]
    fn test_window_split_increases_count() {
        let mut win = w();
        win.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
        assert_eq!(win.pane_count(), 2);
    }

    #[test]
    fn test_window_close_non_last_pane() {
        let mut win = w();
        win.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
        let action = win.close_pane(PaneId(2)).unwrap();
        assert!(matches!(action, WindowAction::PaneClosed));
        assert_eq!(win.pane_count(), 1);
    }

    #[test]
    fn test_window_close_last_pane_returns_empty() {
        let mut win = w();
        let action = win.close_pane(PaneId(1)).unwrap();
        assert!(matches!(action, WindowAction::WindowEmpty));
    }

    #[test]
    fn test_window_close_nonexistent_pane_errors() {
        let mut win = w();
        let err = win.close_pane(PaneId(99)).unwrap_err();
        assert!(matches!(err, WindowError::PaneNotFound(PaneId(99))));
    }

    #[test]
    fn test_window_set_name() {
        let mut win = w();
        win.set_name("renamed");
        assert_eq!(win.name(), "renamed");
    }

    #[test]
    fn test_window_activity() {
        let mut win = w();
        win.set_activity(true);
        assert!(win.has_activity());
        win.set_activity(false);
        assert!(!win.has_activity());
    }

    #[test]
    fn test_window_navigate_two_panes() {
        let mut win = w();
        win.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
        assert_eq!(win.active_pane_id(), PaneId(1));
        win.navigate(Direction::Vertical);
        assert_eq!(win.active_pane_id(), PaneId(2));
    }

    #[test]
    fn test_window_resize_updates_hints() {
        let mut win = w();
        win.resize(160, 48);
        // After resize the layout dimensions should be updated; split
        // validation uses these.  We verify a split still works.
        win.split_active(Direction::Vertical, 0.5, PaneId(2)).unwrap();
        assert_eq!(win.pane_count(), 2);
    }
}
