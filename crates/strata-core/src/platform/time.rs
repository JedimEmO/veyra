//! The one `Instant`: std's natively, `web-time`'s on wasm32, where std's
//! panics because a browser has no monotonic clock behind it.

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;

/// Wall-clock time, for seeds only; `SystemTime::now` panics on wasm32 just
/// as `Instant::now` does.
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_arch = "wasm32")]
pub use web_time::{SystemTime, UNIX_EPOCH};
