//! The one parallel map. Natively it hands work out one item at a time to
//! scoped threads, so a costly item does not leave the other threads idle at
//! the end; on wasm32 there are no threads to spawn and it runs in turn.
//!
//! The results are always in the items' order. A caller that must run one
//! pool at a time takes its own lock around the call: this helper holds none,
//! so a map inside a map of another caller cannot deadlock.

/// The most threads any map in the process takes. Beyond this the
/// development machine's CPU fails under the load (a SIGSEGV with every
/// thread in the sampler), so no caller asks for more.
pub const MAX_THREADS: usize = 16;

/// The threads a map may use: the cores the process has, up to
/// [`MAX_THREADS`]; one on wasm32.
#[must_use]
pub fn threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(MAX_THREADS)
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// `f` over `items` on up to `threads` threads (at least one, at most
/// [`MAX_THREADS`]), the results in the items' order.
pub fn map<I: Sync, T: Send>(threads: usize, items: &[I], f: impl Fn(&I) -> T + Sync) -> Vec<T> {
    map_index(items.len(), threads, |i| f(&items[i]))
}

/// `f` over `0..len` on up to `threads` threads (at least one, at most
/// [`MAX_THREADS`]), the results in index order.
pub fn map_index<T: Send>(len: usize, threads: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let threads = threads.min(len).clamp(1, MAX_THREADS);
        let next = AtomicUsize::new(0);
        let mut done: Vec<(usize, T)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut out = Vec::new();
                        loop {
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            if i >= len {
                                break out;
                            }
                            out.push((i, f(i)));
                        }
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|worker| worker.join().expect("a parallel map thread panicked"))
                .collect()
        });
        done.sort_unstable_by_key(|(i, _)| *i);
        done.into_iter().map(|(_, value)| value).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = threads;
        (0..len).map(f).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_come_back_in_order_for_any_thread_count() {
        let items: Vec<u32> = (0..100).collect();
        for n in [1, 3, 64] {
            let out = map(n, &items, |&x| x * 2);
            assert_eq!(out, items.iter().map(|x| x * 2).collect::<Vec<_>>());
        }
        assert!(map_index(0, 4, |i| i).is_empty());
    }
}
