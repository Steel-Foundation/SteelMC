//! Brushable block entity for archaeology brush progress and delayed loot.

use std::str::FromStr as _;
use std::sync::{Arc, Weak};

use glam::DVec3;
use rand::{SeedableRng as _, rngs::StdRng};
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::{REGISTRY, RegistryExt as _, vanilla_block_entity_types, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction, DowncastType, DowncastTypeKey, Identifier};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::block_entity::BlockEntity;
use crate::entity::{LivingEntity as _, entity_loot_ref};
use crate::player::Player;
use crate::world::World;

const BRUSH_COOLDOWN_TICKS: i64 = 10;
const BRUSH_RESET_TICKS: i64 = 40;
const REQUIRED_BRUSHES: i32 = 10;
const RESET_BRUSH_COUNT_TICKS: i64 = 4;
const BRUSH_COMPLETED_LEVEL_EVENT: i32 = 3008;

/// `LevelChunk::set_block_state` re-locks the same block entity to update its
/// cached state, so callers must not hold that mutex while applying these.
#[derive(Default)]
pub struct BrushableWorldMutation {
    pub set_block: Option<BlockStateId>,
    pub completed_level_event_data: Option<i32>,
    pub drop: Option<(DVec3, ItemStack)>,
}

impl BrushableWorldMutation {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.set_block.is_none() && self.completed_level_event_data.is_none() && self.drop.is_none()
    }

    pub fn apply(self, world: &Arc<World>, pos: BlockPos) {
        if let Some((drop_pos, item)) = self.drop {
            let _ = world.spawn_item_with_velocity(drop_pos, item, DVec3::ZERO);
        }
        if let Some(data) = self.completed_level_event_data {
            world.level_event(BRUSH_COMPLETED_LEVEL_EVENT, pos, data, None);
        }
        if let Some(state) = self.set_block {
            let _ = world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
        }
    }
}

/// Result of one brush attempt, including deferred world work for the caller.
pub struct BrushOutcome {
    pub durability_damage: bool,
    pub mutation: BrushableWorldMutation,
}

/// Stores vanilla archaeology brush progress and delayed loot for brushable blocks.
pub struct BrushableBlockEntity {
    level: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
    brush_count: i32,
    brush_count_resets_at_tick: i64,
    cool_down_ends_at_tick: i64,
    item: ItemStack,
    hit_direction: Option<Direction>,
    loot_table: Option<Identifier>,
    loot_table_seed: i64,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BrushableBlockEntity`.
unsafe impl DowncastType for BrushableBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/brushable");
}

impl BrushableBlockEntity {
    /// Creates a brushable block entity with no active brush progress.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            level,
            pos,
            state,
            removed: false,
            brush_count: 0,
            brush_count_resets_at_tick: 0,
            cool_down_ends_at_tick: 0,
            item: ItemStack::empty(),
            hit_direction: None,
            loot_table: None,
            loot_table_seed: 0,
        }
    }

    /// Applies one vanilla brush attempt.
    /// Returns deferred world mutations that must be applied after the
    /// block-entity mutex is released.
    pub fn brush(
        &mut self,
        game_time: i64,
        world: &Arc<World>,
        player: &Player,
        hit_direction: Direction,
        brush: &ItemStack,
    ) -> BrushOutcome {
        if self.hit_direction.is_none() {
            self.hit_direction = Some(hit_direction);
        }

        self.brush_count_resets_at_tick = game_time + BRUSH_RESET_TICKS;
        if game_time < self.cool_down_ends_at_tick {
            return BrushOutcome {
                durability_damage: false,
                mutation: BrushableWorldMutation::default(),
            };
        }

        self.cool_down_ends_at_tick = game_time + BRUSH_COOLDOWN_TICKS;
        self.unpack_loot_table(world, player, brush);

        let previous_completion_state = self.completion_state();
        self.brush_count += 1;
        if self.brush_count >= REQUIRED_BRUSHES {
            let mutation = self.brushing_completed_mutation();
            self.set_changed();
            return BrushOutcome {
                durability_damage: true,
                mutation,
            };
        }

        world.schedule_block_tick_default(self.pos, self.state.get_block(), 2);
        let mut mutation = BrushableWorldMutation::default();
        let completion_state = self.completion_state();
        if previous_completion_state != completion_state {
            mutation.set_block = Some(self.with_dusted(completion_state));
        }

        self.set_changed();
        BrushOutcome {
            durability_damage: false,
            mutation,
        }
    }

    /// Applies vanilla delayed progress decay after brushing stops.
    pub fn check_reset(&mut self, world: &Arc<World>) -> BrushableWorldMutation {
        let mut mutation = BrushableWorldMutation::default();
        let game_time = world.game_time();

        if self.brush_count != 0 && game_time >= self.brush_count_resets_at_tick {
            let previous_completion_state = self.completion_state();
            self.brush_count = 0.max(self.brush_count - 2);
            let completion_state = self.completion_state();
            if previous_completion_state != completion_state {
                mutation.set_block = Some(self.with_dusted(completion_state));
            }
            self.brush_count_resets_at_tick = game_time + RESET_BRUSH_COUNT_TICKS;
            self.set_changed();
        }

        if self.brush_count == 0 {
            self.hit_direction = None;
            self.brush_count_resets_at_tick = 0;
            self.cool_down_ends_at_tick = 0;
        } else {
            world.schedule_block_tick_default(self.pos, self.state.get_block(), 2);
        }

        mutation
    }

    fn unpack_loot_table(&mut self, _world: &Arc<World>, player: &Player, brush: &ItemStack) {
        if !self.item.is_empty() {
            self.loot_table = None;
            return;
        }

        let Some(loot_table_key) = self.loot_table.take() else {
            return;
        };
        let Some(loot_table) = REGISTRY.loot_tables.by_key(&loot_table_key) else {
            return;
        };

        if self.loot_table_seed == 0 {
            let mut rng = rand::rng();
            self.unpack_loot_items(loot_table, &mut rng, player, brush);
        } else {
            let mut rng = StdRng::seed_from_u64(self.loot_table_seed as u64);
            self.unpack_loot_items(loot_table, &mut rng, player, brush);
        }
    }

    fn unpack_loot_items<R: rand::Rng>(
        &mut self,
        loot_table: LootTableRef,
        rng: &mut R,
        player: &Player,
        brush: &ItemStack,
    ) {
        let mut ctx = LootContext::new(rng)
            .with_luck(player.get_luck())
            .with_tool(brush)
            .with_origin(
                f64::from(self.pos.x()) + 0.5,
                f64::from(self.pos.y()) + 0.5,
                f64::from(self.pos.z()) + 0.5,
            )
            .with_this_entity(entity_loot_ref(player));

        if let Some(item) = loot_table
            .get_random_items(&mut ctx)
            .into_iter()
            .find(|item| !item.is_empty())
        {
            self.item = item;
        }
    }

    fn brushing_completed_mutation(&mut self) -> BrushableWorldMutation {
        let mut mutation = BrushableWorldMutation {
            completed_level_event_data: Some(i32::from(self.state.0)),
            drop: self.take_drop_content(),
            set_block: None,
        };

        let turns_into = BLOCK_BEHAVIORS
            .get_behavior_for_state(self.state)
            .and_then(|behavior| behavior.brushable_data(self.state))
            .map_or(vanilla_blocks::AIR.default_state(), |(turns_into, _, _)| {
                turns_into.default_state()
            });
        mutation.set_block = Some(turns_into);
        mutation
    }

    fn take_drop_content(&mut self) -> Option<(DVec3, ItemStack)> {
        if self.item.is_empty() {
            return None;
        }

        let direction = self.hit_direction.unwrap_or(Direction::Up);
        let drop_pos = direction.relative(self.pos);
        let count = rand::random_range(10..=30).min(self.item.count());
        let dropped = self.item.split(count);
        let pos = DVec3::new(
            f64::from(drop_pos.x()) + 0.5,
            f64::from(drop_pos.y()) + 0.5,
            f64::from(drop_pos.z()) + 0.5,
        );
        Some((pos, dropped))
    }

    fn with_dusted(&mut self, completion_state: i32) -> BlockStateId {
        let state = self
            .state
            .set_value(&BlockStateProperties::DUSTED, completion_state as u8);
        self.state = state;
        state
    }

    const fn completion_state(&self) -> i32 {
        match self.brush_count {
            0 => 0,
            1..=2 => 1,
            3..=5 => 2,
            _ => 3,
        }
    }

    /// Client-only fields (`getUpdateTag`): hit direction byte + optional item.
    fn save_client_data(&self, nbt: &mut NbtCompound) {
        if let Some(direction) = self.hit_direction {
            nbt.insert("hit_direction", direction.get_3d_data_value() as i8);
        }
        if !self.item.is_empty() {
            nbt.insert("item", self.item.to_nbt_tag_ref());
        }
    }
}

impl BlockEntity for BrushableBlockEntity {
    fn get_type(&self) -> BlockEntityTypeRef {
        &vanilla_block_entity_types::BRUSHABLE_BLOCK
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();

        self.loot_table = nbt_view
            .string("LootTable")
            .and_then(|value| Identifier::from_str(&value.to_str()).ok());
        self.loot_table_seed = nbt_view.long("LootTableSeed").unwrap_or(0);

        // Vanilla: LootTable and item are mutually exclusive on load.
        if self.loot_table.is_some() {
            self.item = ItemStack::empty();
        } else {
            self.item = nbt_view
                .compound("item")
                .and_then(|compound| ItemStack::from_borrowed_compound(&compound))
                .unwrap_or_else(ItemStack::empty);
        }

        // Vanilla Direction.LEGACY_ID_CODEC: byte 3D data value.
        self.hit_direction = nbt_view
            .byte("hit_direction")
            .map(|value| Direction::from_3d_data_value(i32::from(value)));
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Vanilla trySaveLootTable: if loot is present, skip item and never write hit_direction.
        if let Some(loot_table) = &self.loot_table {
            nbt.insert("LootTable", loot_table.to_string());
            if self.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", self.loot_table_seed);
            }
            return;
        }

        if !self.item.is_empty() {
            nbt.insert("item", self.item.to_nbt_tag_ref());
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        self.save_client_data(&mut nbt);
        Some(nbt)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Weak;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::vanilla_items;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    fn load_from_owned_nbt(entity: &mut BrushableBlockEntity, nbt: &NbtCompound) {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test nbt should reborrow");
        entity.load_additional(&borrowed);
    }

    fn brushable() -> BrushableBlockEntity {
        init_test_registry();
        BrushableBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 64, 2),
            vanilla_blocks::SUSPICIOUS_SAND.default_state(),
        )
    }

    #[test]
    fn save_loot_table_excludes_item_and_hit_direction() {
        let mut entity = brushable();
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:archaeology/desert_pyramid");
        nbt.insert("LootTableSeed", 42_i64);
        nbt.insert("hit_direction", Direction::North.get_3d_data_value() as i8);
        nbt.insert(
            "item",
            ItemStack::new(&vanilla_items::STICK).to_nbt_tag_ref(),
        );
        load_from_owned_nbt(&mut entity, &nbt);

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);

        assert_eq!(
            saved.string("LootTable").map(ToString::to_string),
            Some("minecraft:archaeology/desert_pyramid".to_owned())
        );
        assert_eq!(saved.long("LootTableSeed"), Some(42));
        assert!(saved.compound("item").is_none());
        assert!(saved.byte("hit_direction").is_none());
    }

    #[test]
    fn save_loot_table_omits_zero_seed() {
        let mut entity = brushable();
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:archaeology/ocean_ruin_warm");
        nbt.insert("LootTableSeed", 0_i64);
        load_from_owned_nbt(&mut entity, &nbt);

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);

        assert_eq!(
            saved.string("LootTable").map(ToString::to_string),
            Some("minecraft:archaeology/ocean_ruin_warm".to_owned())
        );
        assert!(saved.long("LootTableSeed").is_none());
    }

    #[test]
    fn save_item_only_when_no_loot_table() {
        let mut entity = brushable();
        let mut nbt = NbtCompound::new();
        nbt.insert(
            "item",
            ItemStack::new(&vanilla_items::STICK).to_nbt_tag_ref(),
        );
        load_from_owned_nbt(&mut entity, &nbt);

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);

        assert!(saved.string("LootTable").is_none());
        assert!(saved.compound("item").is_some());
    }

    #[test]
    fn load_loot_table_discards_item() {
        let mut entity = brushable();
        let mut nbt = NbtCompound::new();
        nbt.insert("LootTable", "minecraft:archaeology/desert_pyramid");
        nbt.insert(
            "item",
            ItemStack::new(&vanilla_items::STICK).to_nbt_tag_ref(),
        );
        load_from_owned_nbt(&mut entity, &nbt);

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);

        assert!(saved.string("LootTable").is_some());
        assert!(saved.compound("item").is_none());
    }

    #[test]
    fn load_item_when_no_loot_table() {
        let mut entity = brushable();
        let mut nbt = NbtCompound::new();
        nbt.insert(
            "item",
            ItemStack::with_count(&vanilla_items::STICK, 3).to_nbt_tag_ref(),
        );
        load_from_owned_nbt(&mut entity, &nbt);

        assert_eq!(entity.item.count(), 3);
        assert!(entity.item.is(&vanilla_items::STICK));

        let mut saved = NbtCompound::new();
        entity.save_additional(&mut saved);
        assert!(saved.compound("item").is_some());
        assert!(saved.string("LootTable").is_none());
    }

    #[test]
    fn hit_direction_is_byte_on_update_tag_not_disk() {
        let mut entity = brushable();
        let mut nbt = NbtCompound::new();
        nbt.insert("hit_direction", Direction::North.get_3d_data_value() as i8);
        nbt.insert(
            "item",
            ItemStack::new(&vanilla_items::STICK).to_nbt_tag_ref(),
        );
        load_from_owned_nbt(&mut entity, &nbt);

        let mut disk = NbtCompound::new();
        entity.save_additional(&mut disk);
        assert!(disk.byte("hit_direction").is_none());
        assert!(disk.compound("item").is_some());

        let update = entity.get_update_tag().expect("update tag");
        assert_eq!(
            update.byte("hit_direction"),
            Some(Direction::North.get_3d_data_value() as i8)
        );
        assert!(update.compound("item").is_some());
        assert!(update.string("LootTable").is_none());
    }
}
