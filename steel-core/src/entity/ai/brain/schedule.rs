use super::activity::Activity;

const DAY_LENGTH: i64 = 24000;

#[derive(Clone, Copy)]
pub(crate) struct Schedule {
    transitions: &'static [(i32, Activity)],
}

impl Schedule {
    #[must_use]
    pub(crate) const fn new(transitions: &'static [(i32, Activity)]) -> Self {
        Self { transitions }
    }

    #[must_use]
    pub(crate) fn activity_at(&self, day_time: i64) -> Activity {
        let time = day_time.rem_euclid(DAY_LENGTH) as i32;
        self.transitions
            .iter()
            .rev()
            .find(|&&(start, _)| start <= time)
            .or_else(|| self.transitions.last())
            .map_or(Activity::Idle, |&(_, activity)| activity)
    }
}
