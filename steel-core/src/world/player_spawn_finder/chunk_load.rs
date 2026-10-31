use std::sync::Arc;
use std::time::{Duration, Instant};

use steel_utils::ChunkPos;

use crate::chunk::chunk_request::ChunkRequestState;
use crate::world::World;

use super::ChunkRequestHandle;
use super::candidates::CandidateSequence;

/// Bounds the time one poll spends evaluating candidates.
pub(super) struct PollBudget {
    started: Instant,
    limit: Duration,
    evaluated: usize,
}

impl PollBudget {
    pub(super) fn new(limit: Duration) -> Self {
        Self {
            started: Instant::now(),
            limit,
            evaluated: 0,
        }
    }

    /// At least one candidate is evaluated per poll, so the search always moves forward.
    pub(super) fn exhausted(&self) -> bool {
        self.evaluated > 0 && self.started.elapsed() >= self.limit
    }

    pub(super) const fn record_evaluation(&mut self) {
        self.evaluated += 1;
    }
}

/// The chunk the search is currently reading.
pub(super) struct ChunkLoad {
    pub(super) chunk: ChunkPos,
    /// `None` when the chunk is held by the prefetch, which keeps it loaded already.
    request: Option<ChunkRequestHandle>,
    /// Next sequence index to look at. Indices before it need no more work from this chunk.
    pub(super) next_scan: i32,
}

#[derive(Clone, Copy)]
pub(super) enum ChunkLoadState {
    Ready,
    Waiting,
    Cancelled,
}

/// What [`ChunkLoad::scan`] does with a candidate.
pub(super) enum Visit<T> {
    /// The candidate needs no work, so it does not use up the budget.
    Skip,
    /// The candidate was evaluated.
    Evaluated,
    /// The candidate was evaluated and ends the scan with `T`.
    Stop(T),
}

/// How [`ChunkLoad::scan`] ended.
pub(super) enum ScanEnd<T> {
    /// Every remaining candidate in the chunk was visited.
    Finished,
    /// The budget ran out; [`ChunkLoad::scan`] continues from the same place.
    OutOfBudget,
    /// A candidate ended the scan.
    Stopped(T),
}

impl ChunkLoad {
    pub(super) fn open(world: &Arc<World>, chunk: ChunkPos, next_scan: i32, held: bool) -> Self {
        Self {
            chunk,
            request: (!held).then(|| world.request_spawn_candidate_chunk(chunk)),
            next_scan,
        }
    }

    pub(super) fn poll(&self, world: &Arc<World>) -> ChunkLoadState {
        let Some(request) = &self.request else {
            return if world.has_full_chunk(self.chunk) {
                ChunkLoadState::Ready
            } else {
                ChunkLoadState::Waiting
            };
        };
        match request.poll() {
            ChunkRequestState::Cancelled => ChunkLoadState::Cancelled,
            ChunkRequestState::Ready => ChunkLoadState::Ready,
            ChunkRequestState::Pending { .. } => ChunkLoadState::Waiting,
        }
    }

    /// Visits the candidates of this chunk in sequence order, from where the last scan stopped.
    pub(super) fn scan<T>(
        &mut self,
        sequence: &CandidateSequence,
        budget: &mut PollBudget,
        mut visit: impl FnMut(i32, i32, i32) -> Visit<T>,
    ) -> ScanEnd<T> {
        while self.next_scan < sequence.count() {
            if budget.exhausted() {
                return ScanEnd::OutOfBudget;
            }
            let index = self.next_scan;
            self.next_scan += 1;
            if sequence.chunk(index) != self.chunk {
                continue;
            }
            let (x, z) = sequence.column(index);
            match visit(index, x, z) {
                Visit::Skip => {}
                Visit::Evaluated => budget.record_evaluation(),
                Visit::Stop(value) => {
                    budget.record_evaluation();
                    return ScanEnd::Stopped(value);
                }
            }
        }
        ScanEnd::Finished
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::BlockPos;

    use super::*;

    #[test]
    fn scan_resumes_without_skipping_or_repeating_candidates() {
        for (suggestion, radius) in [
            (BlockPos::new(0, 64, 0), 10),
            (BlockPos::new(-1, 64, -1), 40),
            (BlockPos::new(15, 64, 15), 1),
            (BlockPos::new(0, 64, 0), 0),
        ] {
            let Ok(mut sequence) = CandidateSequence::new(suggestion, radius) else {
                panic!("sequence should build");
            };
            for offset in [0, 1, sequence.count() - 1] {
                sequence.offset = offset;
                let expected: Vec<i32> = (0..sequence.count())
                    .filter(|&index| sequence.chunk(index) == sequence.chunk(0))
                    .collect();

                // A zero budget ends every scan after one candidate, so this resumes at each.
                let mut load = ChunkLoad {
                    chunk: sequence.chunk(0),
                    request: None,
                    next_scan: 0,
                };
                let mut visited = Vec::new();
                loop {
                    let mut budget = PollBudget::new(Duration::ZERO);
                    let end = load.scan(&sequence, &mut budget, |index, _, _| {
                        visited.push(index);
                        Visit::<()>::Evaluated
                    });
                    if matches!(end, ScanEnd::Finished) {
                        break;
                    }
                }
                assert_eq!(visited, expected, "radius {radius}, offset {offset}");
            }
        }
    }
}
