/// Actions emitted by the VTE state machine during state transitions.
///
/// Each action represents a semantic event that the [`crate::parser::Performer`]
/// must handle. Actions map directly to the Paul Flo Williams VTE state diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// No action — used for transitions that produce no output.
    None,
    /// Print the accumulated UTF-8 character to the display.
    Print,
    /// Execute a C0 or C1 control function.
    Execute,
    /// Clear the parameter accumulator and intermediates.
    Clear,
    /// Collect an intermediate byte (0x20–0x2F) into the intermediate buffer.
    Collect,
    /// Accumulate a parameter digit or separator.
    Param,
    /// Dispatch a final ESC sequence byte.
    EscDispatch,
    /// Dispatch a final CSI sequence byte.
    CsiDispatch,
    /// Hook into a DCS sequence.
    Hook,
    /// Pass a DCS data byte through.
    Put,
    /// Unhook from a DCS sequence.
    Unhook,
    /// Start collecting an OSC string.
    OscStart,
    /// Accumulate a byte into the OSC string.
    OscPut,
    /// Dispatch the completed OSC string.
    OscEnd,
    /// Ignore the current byte — used in certain error-recovery states.
    Ignore,
}
