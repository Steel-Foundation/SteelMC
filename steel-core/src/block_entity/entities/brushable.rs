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
use crate::player::Player;
use crate::world::World;

const BRUSH_COOLDOWN_TICKS: i64 = 10;
const BRUSH_RESET_TICKS: i64 = 40;
const REQUIRED_BRUSHES: i32 = 10;
const RESET_BRUSH_COUNT_TICKS: i64 = 4;
const BRUSH_COMPLETED_LEVEL_EVENT: i32 = 3008;

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

    /// Applies one vanilla brush attempt and returns whether the brush should lose durability.
    pub fn brush(
        &mut self,
        game_time: i64,
        world: &Arc<World>,
        player: &Player,
        hit_direction: Direction,
        brush: &ItemStack,
    ) -> bool {
        if self.hit_direction.is_none() {
            self.hit_direction = Some(hit_direction);
        }

        self.brush_count_resets_at_tick = game_time + BRUSH_RESET_TICKS;
        if game_time < self.cool_down_ends_at_tick {
            return false;
        }

        self.cool_down_ends_at_tick = game_time + BRUSH_COOLDOWN_TICKS;
        self.unpack_loot_table(world, player, brush);

        let previous_completion_state = self.completion_state();
        self.brush_count += 1;
        if self.brush_count >= REQUIRED_BRUSHES {
            self.brushing_completed(world);
            return true;
        }

        world.schedule_block_tick_default(self.pos, self.state.get_block(), 2);
        let completion_state = self.completion_state();
        if previous_completion_state != completion_state {
            self.update_dusted_state(world, completion_state);
        }

        self.set_changed();
        false
    }

    /// Applies vanilla delayed progress decay after brushing stops.
    pub fn check_reset(&mut self, world: &Arc<World>) {
        if self.brush_count == 0 || world.game_time() < self.brush_count_resets_at_tick {
            return;
        }

        let previous_completion_state = self.completion_state();
        self.brush_count = 0.max(self.brush_count - 2);
        if self.brush_count == 0 {
            self.hit_direction = None;
            self.brush_count_resets_at_tick = 0;
            self.cool_down_ends_at_tick = 0;
        } else {
            self.brush_count_resets_at_tick = world.game_time() + RESET_BRUSH_COUNT_TICKS;
            world.schedule_block_tick_default(self.pos, self.state.get_block(), 2);
        }

        let completion_state = self.completion_state();
        if previous_completion_state != completion_state {
            self.update_dusted_state(world, completion_state);
        }

        self.set_changed();
    }

    fn unpack_loot_table(&mut self, _world: &Arc<World>, _player: &Player, brush: &ItemStack) {
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
            self.unpack_loot_items(loot_table, &mut rng, brush);
        } else {
            let mut rng = StdRng::seed_from_u64(self.loot_table_seed as u64);
            self.unpack_loot_items(loot_table, &mut rng, brush);
        }
    }

    fn unpack_loot_items<R: rand::Rng>(
        &mut self,
        loot_table: LootTableRef,
        rng: &mut R,
        brush: &ItemStack,
    ) {
        // TODO: wire player luck
        let mut ctx = LootContext::new(rng)
            .with_block_state(self.state)
            .with_tool(brush)
            .with_origin(
                f64::from(self.pos.x()),
                f64::from(self.pos.y()),
                f64::from(self.pos.z()),
            );

        if let Some(item) = loot_table
            .get_random_items(&mut ctx)
            .into_iter()
            .find(|item| !item.is_empty())
        {
            self.item = item;
        }
    }

    fn brushing_completed(&mut self, world: &Arc<World>) {
        self.drop_content(world);
        world.level_event(
            BRUSH_COMPLETED_LEVEL_EVENT,
            self.pos,
            i32::from(self.state.0),
            None,
        );

        let turns_into = BLOCK_BEHAVIORS
            .get_behavior_for_state(self.state)
            .and_then(|behavior| behavior.brushable_data(self.state))
            .map_or(vanilla_blocks::AIR.default_state(), |(turns_into, _, _)| {
                turns_into.default_state()
            });

        world.set_block(self.pos, turns_into, UpdateFlags::UPDATE_ALL);
    }

    fn drop_content(&mut self, world: &Arc<World>) {
        if self.item.is_empty() {
            return;
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
        let _ = world.spawn_item_with_velocity(pos, dropped, DVec3::ZERO);
    }

    fn update_dusted_state(&mut self, world: &Arc<World>, completion_state: i32) {
        let state = world
            .get_block_state(self.pos)
            .set_value(&BlockStateProperties::DUSTED, completion_state as u8);
        if world.set_block(self.pos, state, UpdateFlags::UPDATE_ALL) {
            self.state = state;
        }
    }

    const fn completion_state(&self) -> i32 {
        match self.brush_count {
            0 => 0,
            1..=2 => 1,
            3..=5 => 2,
            _ => 3,
        }
    }

    fn load_direction(value: &str) -> Option<Direction> {
        match value {
            "down" => Some(Direction::Down),
            "up" => Some(Direction::Up),
            "north" => Some(Direction::North),
            "south" => Some(Direction::South),
            "west" => Some(Direction::West),
            "east" => Some(Direction::East),
            _ => None,
        }
    }

    const fn direction_name(direction: Direction) -> &'static str {
        match direction {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::North => "north",
            Direction::South => "south",
            Direction::West => "west",
            Direction::East => "east",
        }
    }

    fn save_client_data(&self, nbt: &mut NbtCompound) {
        if let Some(direction) = self.hit_direction {
            nbt.insert("hit_direction", Self::direction_name(direction));
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
        self.hit_direction = nbt_view
            .string("hit_direction")
            .and_then(|value| Self::load_direction(&value.to_str()));
        self.item = nbt_view
            .compound("item")
            .and_then(|compound| ItemStack::from_borrowed_compound(&compound))
            .unwrap_or_else(ItemStack::empty);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(loot_table) = &self.loot_table {
            nbt.insert("LootTable", loot_table.to_string());
            if self.loot_table_seed != 0 {
                nbt.insert("LootTableSeed", self.loot_table_seed);
            }
        }
        self.save_client_data(nbt);
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = NbtCompound::new();
        self.save_client_data(&mut nbt);
        Some(nbt)
    }
}
