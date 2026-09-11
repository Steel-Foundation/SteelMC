use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// A named brain state that gates which behaviors a mob may run.
#[derive(Debug)]
pub struct Activity {
    pub key: Identifier,
}

/// A registered activity.
pub type ActivityRef = &'static Activity;

pub struct ActivityRegistry {
    activities_by_id: Vec<ActivityRef>,
    activities_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ActivityRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            activities_by_id: Vec::new(),
            activities_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    ActivityRegistry,
    ActivityRef,
    activities_by_id,
    activities_by_key,
    allows_registering,
    "Cannot register duplicate activity key: {}"
);

crate::impl_registry!(
    ActivityRegistry,
    Activity,
    activities_by_id,
    activities_by_key,
    activities
);
