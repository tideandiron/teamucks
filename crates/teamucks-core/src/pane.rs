/// Pane entity: ties a PTY to a Terminal and produces frame diffs for clients.
///
/// A [`Pane`] is the central abstraction in the teamucks multiplexer.  It
/// owns a PTY master, a child process, and a [`Terminal`] that interprets the
/// child's output.  It provides:
///
/// - [`Pane::spawn`] — open a PTY, fork the child, initialise the terminal.
/// - [`Pane::feed`] — push PTY output bytes into the terminal.
/// - [`Pane::write_input`] — send user input to the child via PTY master.
/// - [`Pane::resize`] — resize PTY + terminal dimensions.
/// - [`Pane::compute_diff`] — compute incremental frame diff since the last
///   snapshot.
/// - [`Pane::full_frame`] — capture the complete current frame.
/// - [`Pane::try_reap`] — non-blocking waitpid.
use teamucks_vte::terminal::Terminal;

use crate::{
    protocol::{CellData, ColorData, ServerMessage},
    pty::{ChildProcess, ExitStatus, PtyError, PtyMaster},
};

// ---------------------------------------------------------------------------
// PaneId
// ---------------------------------------------------------------------------

/// A unique identifier for a pane within a session.
///
/// # Examples
///
/// ```
/// use teamucks_core::pane::PaneId;
/// let id = PaneId(1);
/// assert_eq!(id, PaneId(1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u32);

impl std::fmt::Display for PaneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pane:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// FrameSnapshot
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of the terminal grid, used for diffing.
///
/// Each cell is stored as a [`CellData`] (grapheme + style) since that is the
/// wire format and avoids an extra conversion step in [`Pane::compute_diff`].
pub(crate) struct FrameSnapshot {
    /// Cell data in row-major order (`row * cols + col`).
    pub(crate) cells: Vec<CellData>,
    /// Number of columns at the time of capture.
    pub(crate) cols: u16,
    /// Number of rows at the time of capture.
    pub(crate) rows: u16,
    /// Cursor column at the time of capture.
    ///
    /// Stored for use by the renderer when positioning the cursor after
    /// completing a frame update.
    pub(crate) cursor_col: u16,
    /// Cursor row at the time of capture.
    pub(crate) cursor_row: u16,
    /// Whether the cursor was visible at the time of capture.
    pub(crate) cursor_visible: bool,
}

// ---------------------------------------------------------------------------
// Pane
// ---------------------------------------------------------------------------

/// A terminal pane: a PTY + child process + terminal emulator.
///
/// The pane is the single source of truth for a running terminal session.
/// It owns the PTY master fd (write input), the child PID (for signals and
/// waitpid), and the [`Terminal`] that interprets the child's byte stream.
pub struct Pane {
    id: PaneId,
    terminal: Terminal,
    pty: PtyMaster,
    child: ChildProcess,
    title: String,
    last_frame: Option<FrameSnapshot>,
}

impl Pane {
    /// Spawn a new pane: open a PTY, fork `command args`, and create a
    /// [`Terminal`] at the given dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError`] if the PTY cannot be opened or the child cannot
    /// be spawned.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use teamucks_core::pane::{Pane, PaneId};
    ///
    /// let pane = Pane::spawn(PaneId(1), 80, 24, "/bin/sh", &[])
    ///     .expect("spawn must succeed in a PTY-capable environment");
    /// assert!(pane.is_alive());
    /// ```
    pub fn spawn(
        id: PaneId,
        cols: u16,
        rows: u16,
        command: &str,
        args: &[&str],
    ) -> Result<Self, PtyError> {
        if cols == 0 || rows == 0 {
            return Err(PtyError::InvalidWindowSize { cols, rows });
        }

        let (pty, slave) = PtyMaster::open()?;
        pty.set_window_size(cols, rows)?;

        let child = ChildProcess::spawn(slave, command, args)?;

        // usize conversion: cols/rows are u16, always fit in usize.
        let terminal = Terminal::new(cols as usize, rows as usize);

        Ok(Self { id, terminal, pty, child, title: String::new(), last_frame: None })
    }

    /// Feed raw PTY output bytes into the terminal emulator.
    ///
    /// This is the hot path: bytes read from the PTY master are forwarded here
    /// so the terminal parser can update the grid.
    pub fn feed(&mut self, data: &[u8]) {
        self.terminal.feed(data);
    }

    /// Write `data` to the PTY master (sends bytes to the child's stdin).
    ///
    /// # Errors
    ///
    /// Returns [`PtyError`] if the write syscall fails.
    pub fn write_input(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut written = 0;
        while written < data.len() {
            let n = self.pty.write(&data[written..])?;
            if n == 0 {
                break; // EOF on master — child closed.
            }
            written += n;
        }
        Ok(())
    }

    /// Resize the PTY and terminal to `cols × rows`.
    ///
    /// Sends `TIOCSWINSZ` to the PTY master (which delivers `SIGWINCH` to the
    /// child) and resizes the internal [`Terminal`] grid.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::InvalidWindowSize`] if either dimension is zero.
    /// Returns [`PtyError::WindowSize`] if the ioctl fails.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.pty.set_window_size(cols, rows)?;
        self.terminal.resize(cols as usize, rows as usize);
        Ok(())
    }

    /// Compute an incremental frame diff against the last snapshot.
    ///
    /// Compares the current terminal grid against `last_frame` (if any) and
    /// returns a [`ServerMessage`].  Updates `last_frame` to the current state.
    ///
    /// - If cell content changed, returns [`ServerMessage::FrameDiff`].
    /// - If only the cursor position or visibility changed (no cell changes),
    ///   returns [`ServerMessage::CursorUpdate`].
    /// - If no previous snapshot exists, returns a full `FrameDiff` covering
    ///   every cell.
    #[must_use]
    pub fn compute_diff(&mut self) -> ServerMessage {
        let current = self.snapshot_current();

        let (diffs, cursor_changed) = if let Some(ref prev) = self.last_frame {
            let diffs = crate::render::diff::compute_diff(prev, &current);
            let cursor_moved = prev.cursor_col != current.cursor_col
                || prev.cursor_row != current.cursor_row
                || prev.cursor_visible != current.cursor_visible;
            (diffs, cursor_moved)
        } else {
            // No previous frame: return a diff covering all cells.
            (crate::render::diff::full_as_diff(&current), false)
        };

        // If the only change is cursor movement, return a CursorUpdate instead
        // of an empty FrameDiff.
        if diffs.is_empty() && cursor_changed {
            let col = current.cursor_col;
            let row = current.cursor_row;
            let visible = current.cursor_visible;
            self.last_frame = Some(current);
            return ServerMessage::CursorUpdate {
                pane_id: self.id.0,
                col,
                row,
                visible,
                shape: crate::protocol::CursorShape::Block,
            };
        }

        self.last_frame = Some(current);
        ServerMessage::FrameDiff { pane_id: self.id.0, diffs }
    }

    /// Capture the complete current frame as a [`ServerMessage::FullFrame`].
    ///
    /// Updates `last_frame` so that subsequent [`compute_diff`] calls are
    /// relative to this snapshot.
    #[must_use]
    pub fn full_frame(&mut self) -> ServerMessage {
        let snap = self.snapshot_current();
        let cells = snap.cells.clone();
        let cols = snap.cols;
        let rows = snap.rows;
        self.last_frame = Some(snap);
        ServerMessage::FullFrame { pane_id: self.id.0, cols, rows, cells }
    }

    /// Return `true` if the child process has not yet exited.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.child.try_wait().is_ok_and(|s| s.is_none())
    }

    /// Non-blocking waitpid.  Returns the exit status if the child has exited,
    /// `None` if it is still running.
    pub fn try_reap(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Return an immutable reference to the internal [`Terminal`].
    #[must_use]
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Return this pane's unique identifier.
    #[must_use]
    pub fn id(&self) -> PaneId {
        self.id
    }

    /// Return the current pane title (set by OSC 0/2 sequences).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    // ---------------------------------------------------------------------------
    // Private helpers
    // ---------------------------------------------------------------------------

    /// Capture the current grid as a [`FrameSnapshot`].
    fn snapshot_current(&self) -> FrameSnapshot {
        let grid = self.terminal.grid();
        let cols = grid.cols();
        let rows = grid.rows();

        let mut cells = Vec::with_capacity(cols * rows);
        for r in 0..rows {
            for c in 0..cols {
                let cell = grid.cell(c, r);
                cells.push(cell_to_data(cell));
            }
        }

        let cursor = grid.cursor();
        // Coordinate values are col/row indices within the grid, which are
        // bounded by u16::MAX (grid dimensions are u16 at the PTY layer).
        // The cast is safe: Terminal::resize accepts usize but is bounded by
        // the u16 passed to Pane::spawn/resize.
        #[allow(clippy::cast_possible_truncation)]
        let cursor_col = cursor.col() as u16;
        #[allow(clippy::cast_possible_truncation)]
        let cursor_row = cursor.row() as u16;

        FrameSnapshot {
            cells,
            #[allow(clippy::cast_possible_truncation)]
            cols: cols as u16,
            #[allow(clippy::cast_possible_truncation)]
            rows: rows as u16,
            cursor_col,
            cursor_row,
            cursor_visible: cursor.is_visible(),
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Convert a [`teamucks_vte::cell::Cell`] to the protocol [`CellData`].
pub(crate) fn cell_to_data(cell: &teamucks_vte::cell::Cell) -> CellData {
    let style = cell.style();
    let fg = color_to_data(style.foreground());
    let bg = color_to_data(style.background());
    let attrs = style.attrs().bits();
    let mut flags: u8 = 0;
    if cell.is_wide() {
        flags |= 0x01;
    }
    if cell.is_continuation() {
        flags |= 0x02;
    }

    CellData { grapheme: cell.grapheme().to_owned(), fg, bg, attrs, flags }
}

/// Convert a [`teamucks_vte::style::Color`] to the protocol [`ColorData`].
#[inline]
pub(crate) fn color_to_data(color: teamucks_vte::style::Color) -> ColorData {
    use teamucks_vte::style::Color;
    match color {
        Color::Default => ColorData::Default,
        Color::Named(idx) | Color::Indexed(idx) => ColorData::Indexed(idx),
        Color::Rgb(r, g, b) => ColorData::Rgb(r, g, b),
    }
}
