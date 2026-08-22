use crate::player::player_data::PersistentStat;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use steel_registry::stat::Stat;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;
use tokio::io;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PlayerStatsFile {
    pub(super) stats: BTreeMap<Identifier, BTreeMap<Identifier, i32>>,
}

impl PlayerStatsFile {
    pub(super) fn from_persistent_stats(persistent_stats: &[PersistentStat]) -> io::Result<Self> {
        let mut stats = BTreeMap::new();

        for PersistentStat { stat, count } in persistent_stats {
            if stats
                .entry(stat.stat_type_key().clone())
                .or_insert_with(BTreeMap::new)
                .insert(stat.stat_value_key().clone(), *count)
                .is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate stat {stat} found in slice"),
                ));
            }
        }

        Ok(Self { stats })
    }

    pub(super) fn into_persistent_stats(self) -> Vec<PersistentStat> {
        let mut stats = Vec::new();

        for (stat_type_key, stats_in_stat_type) in self.stats {
            let Some(stat_type_entry) = REGISTRY.stat_types.by_key(&stat_type_key) else {
                log::warn!(
                    "Player stats file referenced unknown stat type {stat_type_key}, skipping stats in it"
                );
                continue;
            };

            for (stat_value_key, count) in stats_in_stat_type {
                let Some(stat_value) = stat_type_entry.value_from_key(&stat_value_key) else {
                    log::warn!(
                        "Player stats file referenced unknown stat {}.{}:{}.{}, skipping it",
                        stat_type_key.namespace,
                        stat_type_key.path,
                        stat_value_key.namespace,
                        stat_value_key.path
                    );
                    continue;
                };

                stats.push(PersistentStat {
                    stat: Stat::from_erased(stat_type_entry, stat_value),
                    count,
                });
            }
        }

        stats
    }
}

pub(super) fn serialize_player_stats_file(file: &PlayerStatsFile) -> String {
    let mut output = String::new();

    for (stat_type, stats_in_stat_type) in &file.stats {
        output.push_str("[stats.\"");
        output.push_str(&stat_type.namespace);
        output.push(':');
        output.push_str(&stat_type.path);
        output.push_str("\"]\n");

        for (stat_value, count) in stats_in_stat_type {
            output.push('\"');
            output.push_str(&stat_value.namespace);
            output.push(':');
            output.push_str(&stat_value.path);
            output.push_str("\" = ");
            output.push_str(&count.to_string());
            output.push('\n');
        }

        output.push('\n');
    }

    output
}
