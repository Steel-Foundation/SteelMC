use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// A custom stat definition.
#[derive(Debug)]
pub struct CustomStat {
    pub key: Identifier
}

pub type CustomStatRef = &'static CustomStat;

pub struct CustomStatRegistry {
    custom_stats_by_id: Vec<CustomStatRef>,
    custom_stats_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl CustomStatRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            custom_stats_by_id: Vec::new(),
            custom_stats_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    CustomStatRegistry,
    CustomStatRef,
    custom_stats_by_id,
    custom_stats_by_key,
    allows_registering
);

crate::impl_registry!(
    CustomStatRegistry,
    CustomStat,
    custom_stats_by_id,
    custom_stats_by_key,
    custom_stats
);