use std::{io::Cursor, sync::Arc};

use glam::DVec3;
use simdnbt::borrow::read_compound;
use steel_registry::data_components::vanilla_components::ENTITY_DATA;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_utils::nbt::merge_nbt_compounds;
use steel_utils::{BlockPos, WorldAabb, axis::Axis, wrap_degrees};

use super::{AddEntityError, ENTITIES, SharedEntity, next_entity_id};
use crate::physics::{CollisionWorld, WorldCollisionProvider, collide};
use crate::world::World;

/// Vanilla `EntitySpawnReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitySpawnReason {
    Natural,
    ChunkGeneration,
    Spawner,
    Structure,
    Breeding,
    MobSummoned,
    Jockey,
    Event,
    Conversion,
    Reinforcement,
    Triggered,
    Bucket,
    SpawnItemUse,
    Command,
    Dispenser,
    Patrol,
    TrialSpawner,
    Load,
    DimensionTravel,
}

impl EntitySpawnReason {
    #[must_use]
    pub const fn is_spawner(self) -> bool {
        matches!(self, Self::Spawner | Self::TrialSpawner)
    }

    #[must_use]
    pub const fn ignores_light_requirements(self) -> bool {
        matches!(self, Self::TrialSpawner)
    }
}

/// Placement modes used by the shared vanilla entity-spawn coordinator.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EntitySpawnPlacement {
    /// Spawn at a block center, optionally applying vanilla's downward offset.
    Block {
        pos: BlockPos,
        try_move_down: bool,
        moved_up: bool,
    },
    /// Spawn at an exact command or lifecycle position.
    Exact {
        position: DVec3,
        rotation: (f32, f32),
    },
}

impl EntitySpawnPlacement {
    fn factory_position(self) -> DVec3 {
        match self {
            Self::Block {
                pos,
                try_move_down: true,
                ..
            } => DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 1.0,
                f64::from(pos.z()) + 0.5,
            ),
            Self::Block { pos, .. } => DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()),
                f64::from(pos.z()) + 0.5,
            ),
            Self::Exact { position, .. } => position,
        }
    }
}

/// Request for a complete, world-inserting entity spawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntitySpawnRequest<'a> {
    pub entity_type: EntityTypeRef,
    pub placement: EntitySpawnPlacement,
    pub reason: EntitySpawnReason,
    pub finalize_spawn: bool,
    pub play_ambient_sound: bool,
    pub item_stack: Option<&'a ItemStack>,
}

/// Failure from the shared spawn coordinator.
#[derive(Debug)]
pub(crate) enum EntitySpawnError {
    InvalidPosition,
    Peaceful,
    MissingFactory,
    InvalidEntityData,
    AddEntity(AddEntityError),
}

/// Creates an entity instance through the generated entity factory registry.
pub(crate) fn create_entity_instance(
    world: &Arc<World>,
    entity_type: EntityTypeRef,
    position: DVec3,
) -> Result<SharedEntity, EntitySpawnError> {
    if !position.is_finite() || !World::is_in_spawnable_bounds(BlockPos::from(position)) {
        return Err(EntitySpawnError::InvalidPosition);
    }

    if world.difficulty() == steel_utils::types::Difficulty::Peaceful
        && !entity_type.allowed_in_peaceful
    {
        return Err(EntitySpawnError::Peaceful);
    }

    if !ENTITIES.has_factory(entity_type) {
        return Err(EntitySpawnError::MissingFactory);
    }

    ENTITIES
        .create(
            entity_type,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        )
        .ok_or(EntitySpawnError::MissingFactory)
}

/// Inserts a fully initialized entity into the live world entity manager.
pub(crate) fn add_spawned_entity(
    world: &Arc<World>,
    entity: SharedEntity,
) -> Result<(), AddEntityError> {
    world.try_add_entity(entity)
}

/// Applies the implicit entity data carried by an item stack.
pub(crate) fn apply_item_stack_components(
    entity: &SharedEntity,
    item_stack: &ItemStack,
) -> Result<(), EntitySpawnError> {
    let Some(entity_data) = item_stack.get(ENTITY_DATA) else {
        return Ok(());
    };

    if entity_data.entity_type() != entity.entity_type() {
        return Ok(());
    }

    let mut merged = entity.nbt_for_data_compare();
    merge_nbt_compounds(&mut merged, &entity_data.data().copy_tag());

    let mut bytes = Vec::new();
    merged.write(&mut bytes);
    let mut cursor = Cursor::new(bytes.as_slice());
    let borrowed = read_compound(&mut cursor).map_err(|_| EntitySpawnError::InvalidEntityData)?;
    entity.apply_spawn_data((&borrowed).into());
    Ok(())
}

/// Mirrors vanilla `EntityType.spawn` for server-side entity creation.
pub(crate) fn spawn_entity(
    world: &Arc<World>,
    request: EntitySpawnRequest<'_>,
) -> Result<SharedEntity, EntitySpawnError> {
    let entity = create_entity_instance(
        world,
        request.entity_type,
        request.placement.factory_position(),
    )?;

    let (position, rotation) = match request.placement {
        EntitySpawnPlacement::Exact { position, rotation } => (position, rotation),
        EntitySpawnPlacement::Block {
            pos,
            try_move_down,
            moved_up,
        } => {
            let position_above = DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 1.0,
                f64::from(pos.z()) + 0.5,
            );
            if try_move_down {
                entity.base().set_position_local(position_above);
            }

            let y_offset = if try_move_down {
                entity_y_offset(world, pos, moved_up, entity.bounding_box())
            } else {
                0.0
            };
            (
                DVec3::new(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()) + y_offset,
                    f64::from(pos.z()) + 0.5,
                ),
                (wrap_degrees(rand::random::<f32>() * 360.0), 0.0),
            )
        }
    };

    entity.base().set_position_local(position);
    entity.set_rotation(rotation);
    entity.set_old_position_to_current();
    entity.base().set_old_rotation_to_current();

    if request.finalize_spawn
        && let Some(mob) = entity.as_mob()
    {
        mob.set_y_head_rot(rotation.0);
        mob.set_y_body_rot(rotation.0);
        let _ = mob.finalize_spawn(world, request.reason, None);
    }

    if let Some(item_stack) = request.item_stack {
        apply_item_stack_components(&entity, item_stack)?;
    }

    add_spawned_entity(world, Arc::clone(&entity)).map_err(EntitySpawnError::AddEntity)?;

    if request.play_ambient_sound
        && let Some(mob) = entity.as_mob()
    {
        mob.play_ambient_sound();
    }

    Ok(entity)
}

fn entity_y_offset(
    world: &Arc<World>,
    spawn_pos: BlockPos,
    moved_up: bool,
    entity_box: WorldAabb,
) -> f64 {
    let min_y = f64::from(spawn_pos.y()) - if moved_up { 1.0 } else { 0.0 };
    let collision_box = WorldAabb::new(
        f64::from(spawn_pos.x()),
        min_y,
        f64::from(spawn_pos.z()),
        f64::from(spawn_pos.x() + 1),
        f64::from(spawn_pos.y() + 1),
        f64::from(spawn_pos.z() + 1),
    );
    let shapes = WorldCollisionProvider::new(world).get_block_collisions(&collision_box);
    1.0 + collide(
        Axis::Y,
        &entity_box,
        &shapes,
        if moved_up { -2.0 } else { -1.0 },
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpawnGroupData {
    AgeableMob(AgeableMobGroupData),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgeableMobGroupData {
    group_size: i32,
    should_spawn_baby: bool,
    baby_spawn_chance: f32,
}

impl AgeableMobGroupData {
    pub const DEFAULT_BABY_SPAWN_CHANCE: f32 = 0.05;

    #[must_use]
    pub const fn new(should_spawn_baby: bool, baby_spawn_chance: f32) -> Self {
        Self {
            group_size: 0,
            should_spawn_baby,
            baby_spawn_chance,
        }
    }

    #[must_use]
    pub const fn with_should_spawn_baby(should_spawn_baby: bool) -> Self {
        Self::new(should_spawn_baby, Self::DEFAULT_BABY_SPAWN_CHANCE)
    }

    #[must_use]
    pub const fn with_baby_spawn_chance(baby_spawn_chance: f32) -> Self {
        Self::new(true, baby_spawn_chance)
    }

    #[must_use]
    pub const fn group_size(self) -> i32 {
        self.group_size
    }

    #[must_use]
    pub const fn should_spawn_baby(self) -> bool {
        self.should_spawn_baby
    }

    #[must_use]
    pub const fn baby_spawn_chance(self) -> f32 {
        self.baby_spawn_chance
    }

    pub const fn increase_group_size_by_one(&mut self) {
        self.group_size += 1;
    }

    #[must_use]
    pub const fn needs_baby_spawn_roll(self) -> bool {
        self.should_spawn_baby && self.group_size > 0
    }

    pub fn finalize_ageable_spawn(&mut self, baby_roll: impl FnOnce() -> f32) -> bool {
        let spawn_baby = self.needs_baby_spawn_roll() && baby_roll() <= self.baby_spawn_chance;
        self.increase_group_size_by_one();
        spawn_baby
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use simdnbt::owned::NbtCompound;
    use steel_registry::data_components::components::EntityData;
    use steel_registry::data_components::{CustomData, vanilla_components::ENTITY_DATA};
    use steel_registry::init_vanilla_registry;
    use steel_registry::{vanilla_entities, vanilla_items, vanilla_pig_variants};
    use text_components::TextComponent;

    use crate::entity::entities::PigEntity;
    use crate::entity::{AgeableMob, Entity, SharedEntity};

    use super::{AgeableMobGroupData, apply_item_stack_components};

    #[test]
    fn ageable_group_data_increments_before_later_baby_rolls_can_apply() {
        let mut group_data = AgeableMobGroupData::with_should_spawn_baby(true);

        assert!(!group_data.finalize_ageable_spawn(|| {
            panic!("first group member should not roll for baby spawn")
        }));
        assert_eq!(group_data.group_size(), 1);

        assert!(group_data.finalize_ageable_spawn(|| 0.05));
        assert_eq!(group_data.group_size(), 2);
    }

    #[test]
    fn ageable_group_data_can_disable_baby_spawns() {
        let mut group_data = AgeableMobGroupData::with_should_spawn_baby(false);

        assert!(
            !group_data
                .finalize_ageable_spawn(|| { panic!("disabled baby spawning should not roll") })
        );
        assert!(
            !group_data
                .finalize_ageable_spawn(|| { panic!("disabled baby spawning should not roll") })
        );
        assert_eq!(group_data.group_size(), 2);
    }

    #[test]
    fn item_entity_data_overrides_entity_state_without_replacing_defaults() {
        init_vanilla_registry();

        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let entity: SharedEntity = pig.clone();
        pig.set_custom_name(Some(TextComponent::plain("Existing")));

        let mut payload = NbtCompound::new();
        payload.insert("Age", -24_000);
        payload.insert("variant", vanilla_pig_variants::WARM.key.to_string());
        let entity_data = EntityData::new(
            &vanilla_entities::PIG,
            CustomData::try_from_compound(payload).expect("test entity data should be valid"),
        );
        let mut spawn_egg =
            steel_registry::item_stack::ItemStack::new(&vanilla_items::PIG_SPAWN_EGG);
        spawn_egg.set(ENTITY_DATA, entity_data);

        apply_item_stack_components(&entity, &spawn_egg)
            .expect("valid typed entity data should load");

        assert_eq!(pig.get_age(), -24_000);
        assert_eq!(pig.variant().key, vanilla_pig_variants::WARM.key);
        assert!(AgeableMob::is_baby(pig.as_ref()));
        assert_eq!(pig.custom_name(), Some(TextComponent::plain("Existing")));
    }
}
