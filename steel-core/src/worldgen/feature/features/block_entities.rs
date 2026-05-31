use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::PackedBlockPos;

use super::super::prelude::*;
use super::super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(in crate::worldgen::feature) fn set_loot_table_block_entity(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        block_entity_type: BlockEntityTypeRef,
        state: BlockStateId,
        loot_table: &'static str,
        seed: i64,
    ) {
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", loot_table);
        if seed != 0 {
            nbt.insert("LootTableSeed", seed);
        }
        let _ = region.set_block_entity_data(pos, block_entity_type, state, nbt);
    }

    pub(in crate::worldgen::feature) fn set_empty_block_entity(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        block_entity_type: BlockEntityTypeRef,
        state: BlockStateId,
    ) {
        let _ = region.set_block_entity_data(pos, block_entity_type, state, NbtCompound::new());
    }

    pub(in crate::worldgen::feature) fn set_brushable_loot_table(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        loot_table: &'static str,
    ) {
        let seed = PackedBlockPos::from(pos).as_raw();
        Self::set_loot_table_block_entity(
            region,
            pos,
            &vanilla_block_entity_types::BRUSHABLE_BLOCK,
            state,
            loot_table,
            seed,
        );
    }

    pub(in crate::worldgen::feature) fn set_spawner_entity(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        entity_id: &'static str,
    ) {
        let mut entity = NbtCompound::new();
        entity.insert("id", entity_id);

        let mut spawn_data = NbtCompound::new();
        spawn_data.insert("entity", NbtTag::Compound(entity));

        let mut nbt = NbtCompound::new();
        nbt.insert("Delay", 20_i16);
        nbt.insert("MinSpawnDelay", 200_i16);
        nbt.insert("MaxSpawnDelay", 800_i16);
        nbt.insert("SpawnCount", 4_i16);
        nbt.insert("MaxNearbyEntities", 6_i16);
        nbt.insert("RequiredPlayerRange", 16_i16);
        nbt.insert("SpawnRange", 4_i16);
        nbt.insert("SpawnData", NbtTag::Compound(spawn_data));
        nbt.insert(
            "SpawnPotentials",
            NbtTag::List(NbtList::Compound(Vec::new())),
        );

        let _ =
            region.set_block_entity_data(pos, &vanilla_block_entity_types::MOB_SPAWNER, state, nbt);
    }

    pub(in crate::worldgen::feature) fn set_end_gateway_block_entity(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        exit: Option<BlockPos>,
        exact: bool,
    ) {
        let mut nbt = NbtCompound::new();
        nbt.insert("Age", 0_i64);
        if let Some(exit) = exit {
            nbt.insert(
                "exit_portal",
                NbtTag::IntArray(vec![exit.x(), exit.y(), exit.z()]),
            );
        }
        if exact {
            nbt.insert("ExactTeleport", 1_i8);
        }
        let _ =
            region.set_block_entity_data(pos, &vanilla_block_entity_types::END_GATEWAY, state, nbt);
    }

    pub(in crate::worldgen::feature) fn safe_set_feature_block(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> bool {
        if Self::feature_can_replace(region.block_state(pos)) {
            region.set_block_state(pos, state, UpdateFlags::UPDATE_CLIENTS)
        } else {
            false
        }
    }

    pub(in crate::worldgen::feature) fn feature_can_replace(state: BlockStateId) -> bool {
        !state
            .get_block()
            .has_tag(&BlockTag::FEATURES_CANNOT_REPLACE)
    }
}
