use std::sync::atomic::{AtomicU64, Ordering};

use uuid::Uuid;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Generate an opaque process-local identity without imposing identity
/// management on ordinary modeling callers. Persisted parametric integrations
/// retain and restore durable identities through the advanced API.
pub(crate) fn fresh_uuid() -> Uuid {
    let ordinal = u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    Uuid::from_u128(ordinal)
}
