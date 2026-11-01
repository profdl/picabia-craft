/// Why the bridge refused or failed an operation.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BridgeError {
    /// The input crosses itself and others too often (or has too many anchors) to compute live.
    #[error("too complex to compute")]
    TooComplex,
    /// The operation could not run on this input (degenerate, non-finite or open geometry).
    #[error("the operation failed: {0}")]
    Failed(String),
    /// The input image is not usable (byte length does not match its size).
    #[error("bad image: {0}")]
    BadImage(String),
    /// The caller's cancel callback stopped the trace.
    #[error("cancelled")]
    Cancelled,
}
