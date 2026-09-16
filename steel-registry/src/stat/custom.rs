use rustc_hash::FxHashMap;
use steel_utils::{DowncastType, DowncastTypeKey, Identifier};

/// A custom stat definition, which represents a particular stat in the *General* tab in
/// the Statistics menu. This just contains an identifier which identifies the stat.
///
/// These stats are called 'custom' because they are manually defined. Vanilla defines its
/// own custom stats, which have been extracted to generate definitions used by Steel.
///
/// Therefore, 'custom' here should not be misinterpreted to mean Steel's custom stats.
#[derive(Debug)]
pub struct CustomStat {
    pub key: Identifier,
}

pub type CustomStatRef = &'static CustomStat;

/// Contains all the stats that can be found in the *General* tab in the Statistics menu.
///
/// These stats are called 'custom' because they are manually defined. Vanilla defines its
/// own custom stats, which have been extracted to generate definitions used by Steel.
///
/// Therefore, 'custom' here should not be misinterpreted to mean Steel's custom stats.
pub struct CustomStatRegistry {
    custom_stats_by_id: Vec<CustomStatRef>,
    custom_stats_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

// SAFETY: This Steel-owned key uniquely identifies the custom stat registry.
unsafe impl DowncastType for CustomStatRegistry {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:registry/custom_stat");
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
