use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Generate an opaque process-independent identity without imposing identity
/// management on ordinary modeling callers. Persisted parametric integrations
/// may retain and restore the resulting UUID through the advanced API.
pub(crate) fn fresh_uuid() -> Uuid {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let ordinal = u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let process = u128::from(std::process::id());
    Uuid::from_u128(time.rotate_left(29) ^ process.rotate_left(61) ^ ordinal)
}
