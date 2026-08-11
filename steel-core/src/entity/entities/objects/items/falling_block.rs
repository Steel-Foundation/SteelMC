//! Vanilla falling-block entity.

use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::{NbtCompound as BorrowedNbtCompoundView, read_compound};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::CBlockUpdate;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::FallingBlockEntityData;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{
    REGISTRY, vanilla_blocks, vanilla_damage_type_tags, vanilla_damage_types, vanilla_entities,
    vanilla_fluids,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, types::UpdateFlags};

use crate::behavior::blocks::{AnvilBlock, FallingBlock};
use crate::behavior::{BLOCK_BEHAVIORS, BlockPlaceContext, Fallable};
use crate::block_entity::block_state_nbt;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityMovementEmission, EntitySyncedData, RemovalReason,
    next_entity_id,
};
use crate::fluid::fluid_state_to_block;
use crate::physics::MoverType;
use crate::world::{ClipBlockShape, ClipFluid, World};

const DEFAULT_FALL_DAMAGE_PER_DISTANCE: f32 = 0.0;
const DEFAULT_MAX_FALL_DAMAGE: i32 = 40;
const DEFAULT_GRAVITY: f64 = 0.04;
const AIR_DRAG: f64 = 0.98;
const LANDED_HORIZONTAL_DRAG: f64 = 0.7;
const LANDED_VERTICAL_BOUNCE: f64 = -0.5;

struct FallingBlockState {
    block_state: BlockStateId,
    time: i32,
    drop_item: bool,
    cancel_drop: bool,
    hurt_entities: bool,
    fall_damage_max: i32,
    fall_damage_per_distance: f32,
    block_data: Option<NbtCompound>,
}

impl FallingBlockState {
    const fn new(block_state: BlockStateId) -> Self {
        Self {
            block_state,
            time: 0,
            drop_item: true,
            cancel_drop: false,
            hurt_entities: false,
            fall_damage_max: DEFAULT_MAX_FALL_DAMAGE,
            fall_damage_per_distance: DEFAULT_FALL_DAMAGE_PER_DISTANCE,
            block_data: None,
        }
    }
}

/// Entity carrying a block state while it falls under vanilla physics.
#[entity_behavior(class = "FallingBlockEntity")]
pub struct FallingBlockEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<FallingBlockEntityData>,
    state: SyncMutex<FallingBlockState>,
}

// SAFETY: This Steel-owned key uniquely identifies `FallingBlockEntity`.
unsafe impl DowncastType for FallingBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/falling_block");
}

impl FallingBlockEntity {
    /// Creates the default sand-backed entity used by vanilla entity factories.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(FallingBlockEntityData::new()),
            state: SyncMutex::new(FallingBlockState::new(vanilla_blocks::SAND.default_state())),
        }
    }

    fn with_block_state(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        block_state: BlockStateId,
        world: Weak<World>,
    ) -> Self {
        let mut entity_data = FallingBlockEntityData::new();
        entity_data.start_pos.set(BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ));
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(entity_data),
            state: SyncMutex::new(FallingBlockState::new(block_state)),
        }
    }

    /// Creates a falling block from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(FallingBlockEntityData::new()),
            state: SyncMutex::new(FallingBlockState::new(vanilla_blocks::SAND.default_state())),
        }
    }

    /// Replaces a world block with its legacy fluid and spawns its falling entity.
    #[must_use]
    pub fn fall(world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> Arc<Self> {
        let carried_state = if state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            state.set_value(&BlockStateProperties::WATERLOGGED, false)
        } else {
            state
        };
        let entity = Arc::new(Self::with_block_state(
            &vanilla_entities::FALLING_BLOCK,
            next_entity_id(),
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()),
                f64::from(pos.z()) + 0.5,
            ),
            carried_state,
            Arc::downgrade(world),
        ));

        world.set_block(
            pos,
            fluid_state_to_block(state.get_fluid_state()),
            UpdateFlags::UPDATE_ALL,
        );
        if let Err(error) = world.try_add_entity(Arc::clone(&entity) as Arc<dyn Entity>) {
            log::error!("failed to add falling block entity: {error}");
        }
        entity
    }

    /// Returns the carried block state.
    #[must_use]
    pub fn block_state(&self) -> BlockStateId {
        self.state.lock().block_state
    }

    /// Returns the number of falling-block ticks elapsed.
    #[must_use]
    pub fn time(&self) -> i32 {
        self.state.lock().time
    }

    /// Returns the synchronized position where this entity started falling.
    #[must_use]
    pub fn start_pos(&self) -> BlockPos {
        *self.entity_data.lock().start_pos.get()
    }

    /// Enables vanilla falling-block impact damage.
    pub fn set_hurts_entities(&self, damage_per_distance: f32, damage_max: i32) {
        let mut state = self.state.lock();
        state.hurt_entities = true;
        state.fall_damage_per_distance = damage_per_distance;
        state.fall_damage_max = damage_max;
    }

    /// Prevents the carried block from dropping as an item.
    pub fn disable_drop(&self) {
        self.state.lock().cancel_drop = true;
    }

    fn is_concrete_powder(&self) -> bool {
        BLOCK_BEHAVIORS
            .get_behavior(self.block_state().get_block())
            .as_fallable()
            .is_some_and(Fallable::is_concrete_powder)
    }

    fn is_stuck_in_water(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        self.is_concrete_powder() && world.get_block_state(pos).get_fluid_state().is_water()
    }

    fn clip_fast_concrete_into_water(
        &self,
        world: &Arc<World>,
        pos: &mut BlockPos,
        is_stuck_in_water: &mut bool,
    ) {
        if !self.is_concrete_powder() || self.velocity().length_squared() <= 1.0 {
            return;
        }

        let hit = world.clip(
            self.old_position(),
            self.position(),
            ClipBlockShape::Collider,
            ClipFluid::SourceOnly,
        );
        if hit.is_miss()
            || !world
                .get_block_state(hit.block_pos)
                .get_fluid_state()
                .is_water()
        {
            return;
        }

        *pos = hit.block_pos;
        *is_stuck_in_water = true;
    }

    fn may_replace(world: &Arc<World>, pos: BlockPos, current: BlockStateId) -> bool {
        let mut empty = ItemStack::empty();
        let context =
            BlockPlaceContext::directional(world, pos, Direction::Down, &mut empty, Direction::Up);
        BLOCK_BEHAVIORS
            .get_behavior(current.get_block())
            .can_be_replaced(current, &context)
    }

    fn call_on_broken_after_fall(&self, world: &Arc<World>, pos: BlockPos, block: BlockRef) {
        if let Some(fallable) = BLOCK_BEHAVIORS.get_behavior(block).as_fallable() {
            fallable.on_broken_after_fall(world, pos, self);
        }
    }

    fn should_drop_item(&self, world: &Arc<World>) -> bool {
        self.state.lock().drop_item && world.get_game_rule(&ENTITY_DROPS)
    }

    fn drop_block_item(&self, world: &Arc<World>, block: BlockRef) {
        if !self.should_drop_item(world) {
            return;
        }
        let item = REGISTRY.items.by_block(block);
        let _ = self.spawn_at_location(ItemStack::new(item), 0.0);
    }

    fn break_after_failed_placement(&self, world: &Arc<World>, pos: BlockPos, block: BlockRef) {
        self.set_removed(RemovalReason::Discarded);
        if self.should_drop_item(world) {
            self.call_on_broken_after_fall(world, pos, block);
            self.drop_block_item(world, block);
        }
    }

    fn merge_block_entity_data(&self, world: &Arc<World>, pos: BlockPos) {
        if !self.block_state().has_block_entity() {
            return;
        }
        let block_data = self.state.lock().block_data.clone();
        let Some(block_data) = block_data else {
            return;
        };
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };

        let mut merged = block_entity.save_custom_only();
        for (name, tag) in block_data {
            let name_text = name.to_string();
            while merged.remove(&name_text).is_some() {}
            merged.insert(name, tag);
        }

        let mut bytes = Vec::new();
        merged.write(&mut bytes);
        let Ok(borrowed) = read_compound(&mut Cursor::new(bytes.as_slice())) else {
            log::error!("failed to reborrow falling block entity data at {pos:?}");
            return;
        };
        block_entity.load_additional(&borrowed);
        block_entity.set_changed();
    }

    fn try_place_carried_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        current_state: BlockStateId,
        is_stuck_in_water: bool,
        block: BlockRef,
    ) {
        let carried_state = self.block_state();
        let continue_falling = FallingBlock::is_free(world.get_block_state(pos.below()))
            && (!self.is_concrete_powder() || !is_stuck_in_water);
        let survives = BLOCK_BEHAVIORS
            .get_behavior(carried_state.get_block())
            .can_survive(carried_state, world.as_ref(), pos)
            && !continue_falling;

        if !Self::may_replace(world, pos, current_state) || !survives {
            self.break_after_failed_placement(world, pos, block);
            return;
        }

        let placed_state = if carried_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
            && world.get_block_state(pos).get_fluid_state().fluid_id == &vanilla_fluids::WATER
        {
            carried_state.set_value(&BlockStateProperties::WATERLOGGED, true)
        } else {
            carried_state
        };

        if !world.set_block(pos, placed_state, UpdateFlags::UPDATE_ALL) {
            self.break_after_failed_placement(world, pos, block);
            return;
        }

        world.broadcast_to_entity_trackers(
            self.id(),
            CBlockUpdate {
                pos,
                block_state: world.get_block_state(pos),
            },
            None,
        );
        self.set_removed(RemovalReason::Discarded);
        if let Some(fallable) = BLOCK_BEHAVIORS.get_behavior(block).as_fallable() {
            fallable.on_land(world, pos, placed_state, current_state, self);
        }
        self.merge_block_entity_data(world, pos);
    }

    fn tick_server(&self, world: &Arc<World>, block: BlockRef) {
        let mut pos = self.block_position();
        let mut is_stuck_in_water = self.is_stuck_in_water(world, pos);
        self.clip_fast_concrete_into_water(world, &mut pos, &mut is_stuck_in_water);

        if !self.on_ground() && !is_stuck_in_water {
            let time = self.time();
            let outside_expiry_height = pos.y() <= world.get_min_y() || pos.y() > world.get_max_y();
            if time > 600 || time > 100 && outside_expiry_height {
                self.drop_block_item(world, block);
                self.set_removed(RemovalReason::Discarded);
            }
            return;
        }

        let current_state = world.get_block_state(pos);
        let velocity = self.velocity();
        self.set_velocity(DVec3::new(
            velocity.x * LANDED_HORIZONTAL_DRAG,
            velocity.y * LANDED_VERTICAL_BOUNCE,
            velocity.z * LANDED_HORIZONTAL_DRAG,
        ));
        if current_state.get_block() == &vanilla_blocks::MOVING_PISTON {
            return;
        }

        if self.state.lock().cancel_drop {
            self.set_removed(RemovalReason::Discarded);
            self.call_on_broken_after_fall(world, pos, block);
            return;
        }
        self.try_place_carried_block(world, pos, current_state, is_stuck_in_water, block);
    }

    fn is_base_invulnerable_to(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable() && !source.bypasses_invulnerability()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
    }
}

impl Entity for FallingBlockEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        let block_state = self.block_state();
        if block_state.is_air() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }
        let block = block_state.get_block();

        {
            let mut state = self.state.lock();
            state.time = state.time.wrapping_add(1);
        }
        self.apply_gravity();
        let _ = self.move_entity(MoverType::SelfMovement, self.velocity());
        self.apply_effects_from_blocks();
        self.handle_portal();
        if let Some(world) = self.level()
            && self.is_alive()
        {
            self.tick_server(&world, block);
        }
        self.set_velocity(self.velocity() * AIR_DRAG);
    }

    fn get_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn spawn_data(&self) -> i32 {
        i32::from(self.block_state().0)
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn attackable(&self) -> bool {
        false
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::None
    }

    fn hurt(&self, _world: &World, source: &DamageSource, _amount: f32) -> bool {
        if !self.is_base_invulnerable_to(source) {
            self.mark_hurt();
        }
        false
    }

    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        _damage_modifier: f32,
        _source: &DamageSource,
    ) -> bool {
        let (hurt_entities, damage_per_distance, damage_max) = {
            let state = self.state.lock();
            (
                state.hurt_entities,
                state.fall_damage_per_distance,
                state.fall_damage_max,
            )
        };
        if !hurt_entities {
            return false;
        }

        let fall_distance = (fall_distance - 1.0).ceil() as i32;
        if fall_distance < 0 {
            return false;
        }

        let source = BLOCK_BEHAVIORS
            .get_behavior(self.block_state().get_block())
            .as_fallable()
            .map_or_else(
                || {
                    DamageSource::environment(&vanilla_damage_types::FALLING_BLOCK)
                        .with_direct_entity(self.id())
                        .with_causing_entity(self.id())
                },
                |fallable| fallable.get_fall_damage_source(self),
            );
        let damage = (fall_distance as f32 * damage_per_distance)
            .floor()
            .min(damage_max as f32);
        if let Some(world) = self.level() {
            for entity in world.get_entities_in_aabb_matching(&self.bounding_box(), |entity| {
                entity.id() != self.id()
                    && entity.is_living_entity()
                    && entity.is_alive()
                    && !entity.is_spectator()
                    && entity
                        .as_player()
                        .is_none_or(|player| !player.has_infinite_materials())
            }) {
                entity.hurt(&world, &source, damage);
            }
        }

        let block_state = self.block_state();
        if block_state.get_block().has_tag(&BlockTag::ANVIL)
            && damage > 0.0
            && rand::random::<f32>() < 0.05 + fall_distance as f32 * 0.05
        {
            let mut state = self.state.lock();
            if let Some(damaged) = AnvilBlock::damage(state.block_state) {
                state.block_state = damaged;
            } else {
                state.cancel_drop = true;
            }
        }
        false
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert(
            "BlockState",
            NbtTag::Compound(block_state_nbt::save(state.block_state)),
        );
        nbt.insert("Time", state.time);
        nbt.insert("DropItem", i8::from(state.drop_item));
        nbt.insert("HurtEntities", i8::from(state.hurt_entities));
        nbt.insert("FallHurtAmount", state.fall_damage_per_distance);
        nbt.insert("FallHurtMax", state.fall_damage_max);
        if let Some(block_data) = &state.block_data {
            nbt.insert("TileEntityData", NbtTag::Compound(block_data.clone()));
        }
        nbt.insert("CancelDrop", i8::from(state.cancel_drop));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let block_state = nbt
            .compound("BlockState")
            .and_then(block_state_nbt::load)
            .unwrap_or_else(|| vanilla_blocks::SAND.default_state());
        let mut state = self.state.lock();
        state.block_state = block_state;
        state.time = nbt.int("Time").unwrap_or(0);
        state.hurt_entities = nbt.byte("HurtEntities").map_or_else(
            || block_state.get_block().has_tag(&BlockTag::ANVIL),
            |value| value != 0,
        );
        state.fall_damage_per_distance = nbt
            .float("FallHurtAmount")
            .unwrap_or(DEFAULT_FALL_DAMAGE_PER_DISTANCE);
        state.fall_damage_max = nbt.int("FallHurtMax").unwrap_or(DEFAULT_MAX_FALL_DAMAGE);
        state.drop_item = nbt.byte("DropItem").is_none_or(|value| value != 0);
        state.block_data = nbt.compound("TileEntityData").map(|data| data.to_owned());
        state.cancel_drop = nbt.byte("CancelDrop").is_some_and(|value| value != 0);
    }
}

#[cfg(test)]
#[path = "falling_block/tests.rs"]
mod tests;
