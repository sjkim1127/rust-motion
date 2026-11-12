//! Surface allocation and capacity errors.

use std::fmt;

/// A checked surface allocation could not be represented or reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocationError {
    SizeOverflow,
    NotEnoughMemory { bytes: usize },
}

impl fmt::Display for AllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SizeOverflow => f.write_str("requested surface allocation is too large"),
            Self::NotEnoughMemory { bytes } => {
                write!(f, "not enough memory to allocate {} bytes", bytes)
            }
        }
    }
}

impl std::error::Error for AllocationError {}
