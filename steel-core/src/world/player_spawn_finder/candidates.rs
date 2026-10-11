use steel_utils::BlockPos;

const ABSOLUTE_MAX_ATTEMPTS: i32 = 1024;

/// The shuffled order in which the square around the spawn suggestion is searched.
#[derive(Clone, Copy)]
pub(super) struct CandidateSequence {
    spawn_suggestion: BlockPos,
    radius: i32,
    count: i32,
    coprime: i32,
    pub(super) offset: i32,
}

impl CandidateSequence {
    pub(super) fn new(spawn_suggestion: BlockPos, radius: i32) -> Result<Self, String> {
        let square_side = i64::from(radius) * 2 + 1;
        let count = i32::try_from(i64::from(ABSOLUTE_MAX_ATTEMPTS).min(square_side * square_side))
            .map_err(|e| format!("invalid spawn candidate count: {e}"))?;
        Ok(Self {
            spawn_suggestion,
            radius,
            count,
            coprime: get_coprime(count),
            offset: rand::random_range(0..count),
        })
    }

    /// A sequence without candidates, which goes straight to the height fixup.
    pub(super) const fn empty(spawn_suggestion: BlockPos) -> Self {
        Self {
            spawn_suggestion,
            radius: 0,
            count: 0,
            coprime: 0,
            offset: 0,
        }
    }

    pub(super) const fn count(&self) -> i32 {
        self.count
    }

    pub(super) const fn radius(&self) -> i32 {
        self.radius
    }

    /// The block column of the candidate at `index`.
    pub(super) const fn column(&self, index: i32) -> (i32, i32) {
        let value = (self.offset + self.coprime * index) % self.count;
        let side = self.radius * 2 + 1;
        (
            self.spawn_suggestion.x() + value % side - self.radius,
            self.spawn_suggestion.z() + value / side - self.radius,
        )
    }
}

const fn get_coprime(possible_origins: i32) -> i32 {
    if possible_origins <= 16 {
        possible_origins - 1
    } else {
        17
    }
}
