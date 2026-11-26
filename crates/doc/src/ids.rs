//! Process-wide id counters and the typed ids derived from them.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LayerId(pub u64);

impl LayerId {
    pub fn fresh() -> Self {
        LayerId(next_id())
    }
}

/// Identity of the source contents shared by smart-object instances, independent of their bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SmartContentsId(pub u64);

impl SmartContentsId {
    pub fn fresh() -> Self {
        SmartContentsId(next_id())
    }
}

/// Advance the process-wide id counter (shared by layer, document and smart-contents ids) past `max`, so ids
/// loaded from a file never collide with ids minted later.
pub fn ensure_ids_above(max: u64) {
    NEXT_ID.fetch_max(max.saturating_add(1), Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocId(pub u64);

impl DocId {
    pub fn fresh() -> Self {
        DocId(next_id())
    }
}

#[cfg(test)]
mod id_tests {
    #[test]
    fn ensure_ids_above_advances_counter() {
        let far = super::LayerId::fresh().0 + 1000;
        super::ensure_ids_above(far);
        assert!(super::LayerId::fresh().0 > far);
        super::ensure_ids_above(3); // never moves backwards
        assert!(super::DocId::fresh().0 > far);
    }
}
