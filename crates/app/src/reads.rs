//! Turns for the reads that may hold much of a repository in a worker: a
//! use case shares one [`ReadTurns`] among every read of its kind on the
//! daemon, so at most [`READS_AT_ONCE`] run at once and the rest wait.

use futures::future::select_all;
use futures::lock::{Mutex, MutexGuard};

/// How many reads of one kind run at once, by all the daemon's requests
/// together.
pub const READS_AT_ONCE: usize = 2;

/// The turns to read, shared by every read of one kind on the daemon. The
/// rest wait their turn, in no set order; a wait dropped takes no turn.
#[derive(Default)]
pub struct ReadTurns {
    turns: [Mutex<()>; READS_AT_ONCE],
}

impl ReadTurns {
    /// Waits for a turn, held until the guard is dropped.
    pub(crate) async fn turn(&self) -> MutexGuard<'_, ()> {
        // `turns` is never empty, which is all `select_all` asks.
        let (turn, ..) = select_all(self.turns.iter().map(Mutex::lock)).await;
        turn
    }
}
