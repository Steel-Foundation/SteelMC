use std::{collections::BTreeSet, io::Cursor, sync::Arc};

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound, NbtCompound as BorrowedNbtCompoundView, read_compound};
use simdnbt::owned::NbtCompound;
use steel_math::{DEGREE_360, wrap_degrees};
use steel_registry::data_components::vanilla_components::{CUSTOM_DATA, CUSTOM_NAME, ENTITY_DATA};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::nbt::merge_nbt_compounds;
use steel_utils::{BlockPos, Identifier, UuidExt, WorldAabb, axis::Axis, types::Difficulty};
use text_components::TextComponent;
use uuid::Uuid;

use super::{AddEntityError, ENTITIES, SharedEntity, next_entity_id};
use crate::entity::entity::{read_nbt_dvec3, read_nbt_rotation, sanitize_nbt_motion};
use crate::entity::{
    EntityBaseSaveData, EntityFireFreezeState, EntityLoadRequest, start_riding_entities,
};
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
    pub user_is_operator: bool,
}

/// Failure from the shared spawn coordinator.
#[derive(Debug)]
pub(crate) enum EntitySpawnError {
    InvalidPosition,
    Peaceful,
    MissingFactory,
    InvalidEntityData,
    AddEntity,
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

    if world.difficulty() == Difficulty::Peaceful && !entity_type.allowed_in_peaceful {
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

/// Loads a complete entity tree from vanilla entity NBT without publishing it
/// to the world. The caller owns the atomic insertion boundary.
pub(crate) fn load_entity_recursive(
    world: &Arc<World>,
    nbt: &BaseNbtCompound<'_>,
    reason: EntitySpawnReason,
    post_load: impl Fn(&SharedEntity),
) -> Option<SharedEntity> {
    let nbt: BorrowedNbtCompoundView<'_, '_> = nbt.into();
    load_entity_recursive_inner(world, &nbt, reason, &post_load)
}

/// Reborrows an owned compound and loads it through the recursive entity path.
pub(crate) fn load_entity_recursive_owned(
    world: &Arc<World>,
    nbt: &NbtCompound,
    reason: EntitySpawnReason,
    post_load: impl Fn(&SharedEntity),
) -> Option<SharedEntity> {
    let mut bytes = Vec::new();
    nbt.write(&mut bytes);
    let borrowed = read_compound(&mut Cursor::new(bytes.as_slice())).ok()?;
    load_entity_recursive(world, &borrowed, reason, post_load)
}

#[expect(
    clippy::only_used_in_recursion,
    reason = "The spawn reason must be propagated to every nested passenger load."
)]
fn load_entity_recursive_inner(
    world: &Arc<World>,
    nbt: &BorrowedNbtCompoundView<'_, '_>,
    reason: EntitySpawnReason,
    post_load: &impl Fn(&SharedEntity),
) -> Option<SharedEntity> {
    let entity_type = entity_type_from_nbt(nbt)?;
    let position = read_nbt_dvec3(nbt, "Pos").unwrap_or(DVec3::ZERO);
    if !position.is_finite() {
        return None;
    }

    let motion = read_nbt_dvec3(nbt, "Motion").unwrap_or(DVec3::ZERO);
    let rotation = read_nbt_rotation(nbt, "Rotation").unwrap_or((0.0, 0.0));
    if !rotation.0.is_finite() || !rotation.1.is_finite() {
        return None;
    }

    let uuid = nbt
        .int_array("UUID")
        .as_deref()
        .and_then(Uuid::from_int_array)
        .unwrap_or_else(Uuid::new_v4);
    let save_data = load_entity_save_data(nbt);
    let request = EntityLoadRequest {
        entity_type,
        position: super::clamp_loaded_entity_position(position),
        uuid,
        velocity: sanitize_nbt_motion(motion),
        rotation,
        fall_distance: nbt
            .double("fall_distance")
            .or_else(|| nbt.double("FallDistance"))
            .unwrap_or(0.0),
        fire_freeze: EntityFireFreezeState::from_parts(
            read_int(nbt, "Fire").unwrap_or(0),
            read_int(nbt, "TicksFrozen").unwrap_or(0),
            false,
            false,
            nbt.byte("HasVisualFire").is_some_and(|value| value != 0),
        ),
        on_ground: nbt.byte("OnGround").is_some_and(|value| value != 0),
        save_data,
        world: Arc::downgrade(world),
    };

    let entity = ENTITIES.create_and_load_for_spawn_view(request, nbt)?;
    post_load(&entity);

    let Some(passengers) = nbt.list("Passengers") else {
        return Some(entity);
    };
    let passengers = passengers.compounds()?;
    for passenger_nbt in passengers {
        let passenger = load_entity_recursive_inner(world, &passenger_nbt, reason, post_load)?;
        if !start_riding_entities(&passenger, &entity) {
            return None;
        }
    }

    Some(entity)
}

fn entity_type_from_nbt(nbt: &BorrowedNbtCompoundView<'_, '_>) -> Option<EntityTypeRef> {
    let id = nbt.string("id")?.to_str().parse::<Identifier>().ok()?;
    REGISTRY.entity_types.by_key(&id)
}

fn load_entity_save_data(nbt: &BorrowedNbtCompoundView<'_, '_>) -> EntityBaseSaveData {
    let mut save_data = EntityBaseSaveData::new();
    save_data.air_supply = read_int(nbt, "Air").unwrap_or(save_data.air_supply);
    save_data.portal_cooldown = read_int(nbt, "PortalCooldown").unwrap_or(0);
    save_data.no_gravity = nbt.byte("NoGravity").is_some_and(|value| value != 0);
    save_data.invulnerable = nbt.byte("Invulnerable").is_some_and(|value| value != 0);
    save_data.custom_name = nbt
        .get("CustomName")
        .and_then(|tag| TextComponent::from_nbt(&tag.to_owned()));
    save_data.custom_name_visible = nbt
        .byte("CustomNameVisible")
        .is_some_and(|value| value != 0);
    save_data.silent = nbt.byte("Silent").is_some_and(|value| value != 0);
    save_data.glowing = nbt.byte("Glowing").is_some_and(|value| value != 0);
    if let Some(tags) = nbt.list("Tags").and_then(|list| list.strings()) {
        save_data.tags = tags
            .iter()
            .take(super::MAX_ENTITY_TAGS)
            .map(|tag| tag.to_str().into_owned())
            .collect::<BTreeSet<_>>();
    }
    save_data.custom_data = nbt
        .compound("data")
        .map(|data| data.to_owned())
        .unwrap_or_default();
    save_data
}

fn read_int(nbt: &BorrowedNbtCompoundView<'_, '_>, key: &str) -> Option<i32> {
    nbt.int(key)
        .or_else(|| nbt.short(key).map(i32::from))
        .or_else(|| nbt.byte(key).map(i32::from))
}

/// Applies the implicit entity data carried by an item stack.
pub(crate) fn apply_implicit_item_stack_components(entity: &SharedEntity, item_stack: &ItemStack) {
    entity.apply_implicit_item_components(item_stack);

    if let Some(custom_name) = item_stack.get(CUSTOM_NAME) {
        entity.set_custom_name(Some(custom_name.clone()));
    }

    if let Some(custom_data) = item_stack.get(CUSTOM_DATA) {
        entity.set_custom_data(custom_data.copy_tag());
    }
}

/// Applies all entity data carried by an item stack during a normal item spawn.
pub(crate) fn apply_item_stack_components(
    entity: &SharedEntity,
    item_stack: &ItemStack,
    user_is_operator: bool,
) -> Result<(), EntitySpawnError> {
    apply_implicit_item_stack_components(entity, item_stack);

    let Some(entity_data) = item_stack.get(ENTITY_DATA) else {
        return Ok(());
    };

    if entity_data.entity_type() != entity.entity_type() {
        return Ok(());
    }

    if entity.entity_type().only_op_can_set_nbt && !user_is_operator {
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

    let EntitySpawnPlacement::Block {
        pos,
        try_move_down,
        moved_up,
    } = request.placement;
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
    let position = DVec3::new(
        f64::from(pos.x()) + 0.5,
        f64::from(pos.y()) + y_offset,
        f64::from(pos.z()) + 0.5,
    );
    let rotation = (wrap_degrees(rand::random::<f32>() * DEGREE_360), 0.0);

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
        apply_item_stack_components(&entity, item_stack, request.user_is_operator)?;
    }

    add_spawned_entity(world, Arc::clone(&entity)).map_err(|_| EntitySpawnError::AddEntity)?;

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
    use simdnbt::owned::{NbtCompound, NbtList};
    use steel_registry::data_components::CustomData;
    use steel_registry::data_components::components::EntityData;
    use steel_registry::data_components::vanilla_components::{
        CHICKEN_SOUND_VARIANT, CHICKEN_VARIANT, COW_SOUND_VARIANT, COW_VARIANT, ENTITY_DATA,
        PIG_VARIANT, SHEEP_COLOR,
    };
    use steel_registry::init_vanilla_registry;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        DyeColor, RegistryReference, vanilla_chicken_sound_variants, vanilla_chicken_variants,
        vanilla_cow_sound_variants, vanilla_cow_variants, vanilla_entities, vanilla_items,
        vanilla_pig_variants,
    };
    use text_components::TextComponent;

    use crate::entity::entities::{ChickenEntity, CowEntity, PigEntity, SheepEntity};
    use crate::entity::{
        AgeableMob, Entity, EntitySpawnReason, SharedEntity, init_entities,
        load_entity_recursive_owned,
    };
    use crate::test_support::fresh_test_world;

    use super::{AgeableMobGroupData, apply_item_stack_components};

    fn entity_nbt(id: &str) -> NbtCompound {
        let mut entity = NbtCompound::new();
        entity.insert("id", id);
        entity
    }

    #[test]
    fn recursive_spawner_load_rejects_unknown_and_unimplemented_entities() {
        init_vanilla_registry();
        init_entities();
        let world = fresh_test_world("spawner_recursive_load_rejects_unsupported");

        let unknown = entity_nbt("minecraft:not_an_entity");
        assert!(
            load_entity_recursive_owned(&world, &unknown, EntitySpawnReason::Spawner, |_| {},)
                .is_none()
        );

        let malformed = entity_nbt("not an entity identifier");
        assert!(
            load_entity_recursive_owned(&world, &malformed, EntitySpawnReason::Spawner, |_| {},)
                .is_none()
        );

        let unimplemented = entity_nbt("minecraft:blaze");
        assert!(load_entity_recursive_owned(
            &world,
            &unimplemented,
            EntitySpawnReason::Spawner,
            |_| {},
        )
        .is_none());
    }

    #[test]
    fn recursive_spawner_load_rejects_an_unsupported_passenger_tree() {
        init_vanilla_registry();
        init_entities();
        let world = fresh_test_world("spawner_recursive_load_rejects_passenger");

        let mut root = entity_nbt("minecraft:pig");
        root.insert(
            "Passengers",
            NbtList::Compound(vec![entity_nbt("minecraft:blaze")]),
        );

        assert!(
            load_entity_recursive_owned(&world, &root, EntitySpawnReason::Spawner, |_| {},)
                .is_none()
        );
    }

    #[test]
    fn recursive_spawner_load_accepts_a_registered_entity() {
        init_vanilla_registry();
        init_entities();
        let world = fresh_test_world("spawner_recursive_load_accepts_supported");
        let pig = entity_nbt("minecraft:pig");

        assert!(
            load_entity_recursive_owned(&world, &pig, EntitySpawnReason::Spawner, |_| {},)
                .is_some()
        );
    }

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
        payload.insert("Age", pig.get_baby_start_age());
        payload.insert("variant", vanilla_pig_variants::WARM.key.to_string());
        let entity_data = EntityData::new(
            &vanilla_entities::PIG,
            CustomData::try_from_compound(payload).expect("test entity data should be valid"),
        );
        let mut spawn_egg = ItemStack::new(&vanilla_items::PIG_SPAWN_EGG);
        spawn_egg.set(ENTITY_DATA, entity_data);

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid typed entity data should load");

        assert_eq!(pig.get_age(), pig.get_baby_start_age());
        assert_eq!(pig.variant().key, vanilla_pig_variants::WARM.key);
        assert!(AgeableMob::is_baby(pig.as_ref()));
        assert_eq!(pig.custom_name(), Some(TextComponent::plain("Existing")));
    }

    #[test]
    fn item_pig_variant_overrides_spawn_variant() {
        init_vanilla_registry();

        let pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        pig.set_variant(&vanilla_pig_variants::WARM);
        let entity: SharedEntity = pig.clone();

        let mut spawn_egg = ItemStack::new(&vanilla_items::PIG_SPAWN_EGG);
        spawn_egg.set(
            PIG_VARIANT,
            RegistryReference::new(&vanilla_pig_variants::COLD),
        );

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid pig variant component should apply");

        assert_eq!(pig.variant().key, vanilla_pig_variants::COLD.key);
    }

    #[test]
    fn item_cow_components_override_spawn_state() {
        init_vanilla_registry();

        let cow = Arc::new(CowEntity::new(
            &vanilla_entities::COW,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let entity: SharedEntity = cow.clone();

        let mut spawn_egg = ItemStack::new(&vanilla_items::COW_SPAWN_EGG);
        spawn_egg.set(
            COW_VARIANT,
            RegistryReference::new(&vanilla_cow_variants::COLD),
        );
        spawn_egg.set(
            COW_SOUND_VARIANT,
            RegistryReference::new(&vanilla_cow_sound_variants::MOODY),
        );

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid cow components should apply");

        assert_eq!(cow.variant().key, vanilla_cow_variants::COLD.key);
        assert_eq!(
            cow.sound_variant().key,
            vanilla_cow_sound_variants::MOODY.key
        );
    }

    #[test]
    fn item_chicken_components_override_spawn_state() {
        init_vanilla_registry();

        let chicken = Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let entity: SharedEntity = chicken.clone();

        let mut spawn_egg = ItemStack::new(&vanilla_items::CHICKEN_SPAWN_EGG);
        spawn_egg.set(
            CHICKEN_VARIANT,
            RegistryReference::new(&vanilla_chicken_variants::COLD),
        );
        spawn_egg.set(
            CHICKEN_SOUND_VARIANT,
            RegistryReference::new(&vanilla_chicken_sound_variants::PICKY),
        );

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid chicken components should apply");

        assert_eq!(chicken.variant().key, vanilla_chicken_variants::COLD.key);
        assert_eq!(
            chicken.sound_variant().key,
            vanilla_chicken_sound_variants::PICKY.key
        );
    }

    #[test]
    fn item_sheep_color_overrides_spawn_state() {
        init_vanilla_registry();

        let sheep = Arc::new(SheepEntity::new(
            &vanilla_entities::SHEEP,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let entity: SharedEntity = sheep.clone();

        let mut spawn_egg = ItemStack::new(&vanilla_items::SHEEP_SPAWN_EGG);
        spawn_egg.set(SHEEP_COLOR, DyeColor::Pink);

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid sheep color should apply");

        assert_eq!(sheep.color(), DyeColor::Pink);
    }
}
