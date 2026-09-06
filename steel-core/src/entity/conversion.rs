//! Vanilla-shaped mob conversion foundations (`ConversionParams`, `ConversionType`).
//!
//! Mirrors the `world/entity/ConversionParams.java` and `ConversionType.java`
//! records that back `Mob.convertTo`. A conversion replaces (or splits) one mob
//! with a fresh instance of another `EntityType`, transferring the shared state
//! every converted mob inherits.

use std::sync::Arc;

use steel_registry::entity_type::EntityTypeRef;
use steel_utils::types::Difficulty;

use crate::entity::registry::ENTITIES;
use crate::entity::{
    Entity, Mob, RemovalReason, SharedEntity, next_entity_id, start_riding_entities,
};
use crate::inventory::equipment::EquipmentSlot;

/// Vanilla `ConversionParams.AfterConversion`: finalizes a freshly converted
/// mob, mirroring the vanilla functional interface.
pub type AfterConversion<'a> = dyn Fn(&dyn Mob) + 'a;

/// Vanilla `ConversionParams`: how a converted mob should inherit the original.
///
/// Vanilla's record also carries the original mob's `PlayerTeam`; Steel has no
/// scoreboard-team foundation yet, so team transfer is not part of this struct
/// (documented gap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionParams {
    /// Vanilla `ConversionParams.type`.
    pub conversion_type: ConversionType,
    /// Vanilla `ConversionParams.keepEquipment`.
    pub keep_equipment: bool,
    /// Vanilla `ConversionParams.preserveCanPickUpLoot`.
    pub preserve_can_pick_up_loot: bool,
}

impl ConversionParams {
    /// Vanilla `ConversionParams.single`: a single-mob conversion replacing the
    /// original in place. Vanilla also captures the source mob's team here;
    /// Steel has no scoreboard-team foundation yet (documented gap).
    #[must_use]
    pub const fn single(keep_equipment: bool, preserve_can_pick_up_loot: bool) -> Self {
        Self {
            conversion_type: ConversionType::Single,
            keep_equipment,
            preserve_can_pick_up_loot,
        }
    }
}

/// Vanilla `ConversionType`: how `Mob.convertTo` replaces the source mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionType {
    /// The converted mob replaces the original in place (`SINGLE`).
    Single,
    /// The original keeps living and the converted mob spawns separately
    /// (`SPLIT_ON_DEATH`; used by slime splits).
    SplitOnDeath,
}

impl ConversionType {
    /// Returns vanilla `ConversionType.shouldDiscardAfterConversion`.
    #[must_use]
    pub const fn should_discard_after_conversion(self) -> bool {
        matches!(self, Self::Single)
    }

    /// Vanilla `ConversionType.convert`: transfers the conversion state from
    /// `from` onto the freshly created `to`.
    fn convert(self, from: &dyn Mob, to: &SharedEntity, params: ConversionParams) {
        let Some(to_mob) = to.as_mob() else {
            log::error!(
                "cannot convert {} to {}: created entity is not a mob",
                from.entity_type().key,
                to.entity_type().key
            );
            return;
        };
        match self {
            Self::Single => convert_single(from, to_mob, to, params),
            Self::SplitOnDeath => convert_split_on_death(from, to_mob, params),
        }
    }
}

/// Vanilla `Mob.convertTo`: creates a fresh `entity_type` mob, transfers the
/// shared conversion state, runs `after_conversion`, adds the new mob to the
/// world, and discards `from` for single-mob conversions.
///
/// Returns `None` when `from` is removed, the world is gone, `entity_type` is
/// not allowed on the current difficulty (vanilla `EntityType.canSpawn`), has no
/// registered factory, the created entity is not a mob, or the world rejects the
/// new entity; the source mob is left untouched in those cases.
pub(crate) fn convert_to(
    from: &dyn Mob,
    entity_type: EntityTypeRef,
    params: ConversionParams,
    after_conversion: Option<&AfterConversion<'_>>,
) -> Option<SharedEntity> {
    // Vanilla `Mob.convertTo`: a removed mob cannot convert.
    if from.is_removed() {
        return None;
    }
    let world = from.level()?;
    // Vanilla `EntityType.canSpawn` gates `convertTo`'s `entityType.create`:
    // types that are not allowed on Peaceful difficulty must not be created.
    // Steel has no feature-flag foundation yet, so only the peaceful clause of
    // `canSpawn` is checked here.
    if !entity_type.allowed_in_peaceful && world.difficulty() == Difficulty::Peaceful {
        return None;
    }
    let Some(to) = ENTITIES.create(
        entity_type,
        next_entity_id(),
        from.position(),
        Arc::downgrade(&world),
    ) else {
        log::warn!(
            "cannot convert {} to {}: no entity factory is registered",
            from.entity_type().key,
            entity_type.key
        );
        return None;
    };
    let Some(to_mob) = to.as_mob() else {
        log::error!(
            "cannot convert {} to {}: created entity is not a mob",
            from.entity_type().key,
            entity_type.key
        );
        return None;
    };

    params.conversion_type.convert(from, &to, params);
    if let Some(after_conversion) = after_conversion {
        after_conversion(to_mob);
    }

    if let Err(error) = world.try_add_entity(to.clone()) {
        log::error!(
            "failed to add converted {} for {}: {error}",
            to_mob.entity_type().key,
            from.entity_type().key
        );
        return None;
    }

    if params.conversion_type.should_discard_after_conversion() {
        from.set_removed(RemovalReason::Discarded);
    }
    Some(to)
}

/// Vanilla `ConversionType.SINGLE.convert`, in statement order.
fn convert_single(from: &dyn Mob, to: &dyn Mob, to_arc: &SharedEntity, params: ConversionParams) {
    let root_passenger = from.first_passenger();

    copy_position(from, to);
    to.set_velocity(from.velocity());

    if let Some(root_passenger) = root_passenger {
        root_passenger.stop_riding();
        root_passenger.base().set_boarding_cooldown(0);

        for passenger in to_arc.passengers() {
            passenger.stop_riding();
            passenger.set_removed(RemovalReason::Discarded);
        }

        start_riding_entities(&root_passenger, to_arc);
    }

    if let Some(vehicle) = from.vehicle() {
        from.stop_riding();
        start_riding_entities(to_arc, &vehicle);
    }

    if params.keep_equipment {
        for slot in EquipmentSlot::ALL {
            let stack = from.equipment_in_slot(slot);
            if !stack.is_empty() {
                to.living_base().equipment().lock().set(slot, stack);
                to.set_equipment_drop_chance(slot, from.equipment_drop_chance(slot));
            }
        }
    }

    to.set_fall_distance(from.fall_distance());
    to.set_shared_fall_flying(from.is_fall_flying());

    // Vanilla copies the raw `lastHurtByPlayerMemoryTime` field; Steel stores it
    // together with the attacking player's UUID, so it is copied as a pair.
    if let Some(uuid) = from.last_hurt_by_player_uuid() {
        to.set_last_hurt_by_player(uuid, from.last_hurt_by_player_memory_time());
    }
    // Vanilla also copies `hurtTime` (the visual hurt-flash counter); Steel has
    // no equivalent field yet, so it is left out (documented gap).

    to.set_y_body_rot(from.y_body_rot());
    to.set_on_ground(from.on_ground());
    if let Some(bed_position) = from.sleeping_pos() {
        to.set_sleeping_pos(bed_position);
    }
    if let Some(leash_holder) = from.leash_holder() {
        to.set_leashed_to(&leash_holder);
    }

    convert_common(from, to, params);
}

/// Vanilla `ConversionType.SPLIT_ON_DEATH.convert`, in statement order.
fn convert_split_on_death(from: &dyn Mob, to: &dyn Mob, params: ConversionParams) {
    if let Some(root_passenger) = from.first_passenger() {
        root_passenger.stop_riding();
    }
    if from.leash_holder().is_some() {
        from.drop_leash();
    }

    convert_common(from, to, params);
}

/// Vanilla `ConversionType.convertCommon`, in statement order.
fn convert_common(from: &dyn Mob, to: &dyn Mob, params: ConversionParams) {
    to.set_absorption_amount(from.living_base().absorption_amount());

    for effect in from.active_mob_effects() {
        to.add_mob_effect(effect);
    }

    // Vanilla `Mob.setBaby` is a no-op for non-ageable mobs; Steel mirrors that
    // by only forwarding to ageable targets.
    if from.is_baby()
        && let Some(to_ageable) = to.as_ageable_mob()
    {
        to_ageable.set_baby(true);
    }

    if let (Some(old_ageable), Some(converted_ageable)) =
        (from.as_ageable_mob(), to.as_ageable_mob())
    {
        converted_ageable.set_age(old_ageable.get_age());
        converted_ageable.set_forced_age(old_ageable.forced_age());
        converted_ageable.set_forced_age_timer(old_ageable.forced_age_timer());
    }

    // Vanilla copies the `ANGRY_AT` brain memory here; Steel's Brain foundation
    // is still in flight, so the memory transfer is a documented gap.

    if params.preserve_can_pick_up_loot {
        to.set_can_pick_up_loot(from.can_pick_up_loot());
    }

    to.set_left_handed(from.is_left_handed());
    to.set_no_ai(from.is_no_ai());
    if from.is_persistence_required() {
        to.set_persistence_required();
    }

    to.set_custom_name_visible(from.is_custom_name_visible());
    // Vanilla `setSharedFlagOnFire(from.isOnFire())`: keep the fire visual
    // state rather than restarting an ignition timer.
    to.base().set_visual_fire(from.is_on_fire());
    to.set_invulnerable(from.is_invulnerable());
    to.set_no_gravity(from.is_no_gravity());
    to.set_portal_cooldown(from.portal_cooldown());
    to.set_silent(from.is_silent());
    for tag in from.tags() {
        to.add_tag(tag);
    }

    // Vanilla copies the `CUSTOM_NAME` and `CUSTOM_DATA` data components.
    to.set_custom_name(from.custom_name());
    to.set_custom_data(from.custom_data());

    // Vanilla moves `to` onto `from`'s scoreboard team; Steel has no
    // scoreboard-team foundation yet (documented gap). Vanilla's zombie
    // door-breaking copy is Zombie-specific and stays out of this foundation.
}

/// Vanilla `Entity.copyPosition`: copies position and rotation.
fn copy_position(from: &dyn Entity, to: &dyn Entity) {
    if let Err(error) = to.try_set_position(from.position()) {
        log::warn!(
            "failed to copy conversion position {} to {}: {error}",
            from.entity_type().key,
            to.entity_type().key
        );
    }
    to.set_rotation(from.rotation());
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        init_vanilla_registry, vanilla_attributes, vanilla_entities, vanilla_items,
        vanilla_mob_effects,
    };
    use steel_utils::Downcast as _;
    use steel_utils::types::Difficulty;
    use steel_utils::{ChunkPos, Identifier};
    use text_components::TextComponent;

    use super::{ConversionParams, ConversionType};
    use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
    use crate::entity::entities::PigEntity;
    use crate::entity::{
        AgeableMob, Mob, MobEffectInstance, SharedEntity, init_entities, next_entity_id,
    };
    use crate::inventory::equipment::EquipmentSlot;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;

    fn conversion_world() -> Arc<World> {
        let world = fresh_test_world("conversion");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        world
    }

    fn add_pig(world: &Arc<World>) -> SharedEntity {
        let pig: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(1.0, 64.0, 1.0),
            Arc::downgrade(world),
        ));
        world
            .try_add_entity(Arc::clone(&pig))
            .expect("test pig should attach to the loaded chunk");
        pig
    }

    #[test]
    fn single_conversion_transfers_shared_state_and_discards_source() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();
        let pig = add_pig(&world);
        let pig_mob = pig.as_mob().expect("pig is a mob");

        pig_mob.set_rotation((45.0, -10.0));
        pig_mob.set_velocity(DVec3::new(0.5, 0.0, -0.25));
        pig_mob.set_fall_distance(2.25);
        pig_mob.set_on_ground(true);
        pig_mob.set_left_handed(true);
        pig_mob.set_no_ai(true);
        pig_mob.set_persistence_required();
        pig_mob.set_custom_name(Some(TextComponent::plain("converted")));
        pig_mob.set_custom_name_visible(true);
        pig_mob.set_invulnerable(true);
        pig_mob.set_no_gravity(true);
        pig_mob.set_portal_cooldown(7);
        pig_mob.set_silent(true);
        pig_mob.add_tag("audit_tag".to_owned());
        pig_mob.set_can_pick_up_loot(true);
        // The modifier lets the source pig hold absorption despite its zero
        // vanilla max; the ABSORPTION effect gives the converted cow headroom
        // after the effects are copied.
        pig_mob.attributes().lock().add_modifier(
            vanilla_attributes::MAX_ABSORPTION,
            AttributeModifier {
                id: Identifier::vanilla_static("conversion_test_absorption"),
                amount: 10.0,
                operation: AttributeModifierOperation::AddValue,
            },
            true,
        );
        pig_mob.add_mob_effect(MobEffectInstance::new(vanilla_mob_effects::SPEED, 1));
        pig_mob.add_mob_effect(MobEffectInstance::new(vanilla_mob_effects::ABSORPTION, 1));
        pig_mob.set_absorption_amount(3.5);
        assert_eq!(
            pig_mob.living_base().absorption_amount(),
            3.5,
            "the source pig should hold its absorption before conversion"
        );
        pig_mob.set_equipment_drop_chance(EquipmentSlot::Head, 0.5);
        pig_mob.living_base().equipment().lock().set(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::IRON_HELMET),
        );
        let pig_ageable = pig.as_ageable_mob().expect("pig is ageable");
        pig_ageable.set_age(-24_000);
        pig_ageable.set_forced_age(12);
        pig_ageable.set_forced_age_timer(3);

        let cow = pig
            .downcast_ref::<PigEntity>()
            .expect("pig is concrete")
            .convert_to(
                &vanilla_entities::COW,
                ConversionParams::single(true, true),
                None,
            )
            .expect("pig should convert to cow");

        assert_eq!(cow.entity_type(), &vanilla_entities::COW);
        assert!(pig.is_removed(), "single conversion discards the source");
        assert!(
            world.get_entity_by_id(cow.id()).is_some(),
            "the converted mob joins the world"
        );
        assert_eq!(cow.position(), DVec3::new(1.0, 64.0, 1.0));
        assert_eq!(cow.rotation(), (45.0, -10.0));
        assert_eq!(cow.velocity(), DVec3::new(0.5, 0.0, -0.25));
        assert_eq!(cow.fall_distance(), 2.25);
        assert!(cow.on_ground());

        let cow_mob = cow.as_mob().expect("cow is a mob");
        assert!(cow_mob.is_left_handed());
        assert!(cow_mob.is_no_ai());
        assert!(cow_mob.is_persistence_required());
        assert_eq!(
            cow_mob.custom_name(),
            Some(TextComponent::plain("converted"))
        );
        assert!(cow_mob.is_custom_name_visible());
        assert!(cow_mob.is_invulnerable());
        assert!(cow_mob.is_no_gravity());
        assert_eq!(cow_mob.portal_cooldown(), 7);
        assert!(cow_mob.is_silent());
        assert!(cow_mob.tags().contains(&"audit_tag".to_owned()));
        assert!(cow_mob.can_pick_up_loot());
        // Vanilla `convertCommon` copies absorption before the effects, so the
        // target's pre-effect max absorption (0 for a cow) clamps the copy; the
        // ABSORPTION effect is still transferred and restores absorption over
        // its effect ticks, exactly as in vanilla.
        assert_eq!(cow_mob.living_base().absorption_amount(), 0.0);
        let effects = cow_mob.active_mob_effects();
        assert!(effects.contains(&MobEffectInstance::new(vanilla_mob_effects::SPEED, 1)));
        assert!(effects.contains(&MobEffectInstance::new(vanilla_mob_effects::ABSORPTION, 1)));
        assert_eq!(cow_mob.equipment_drop_chance(EquipmentSlot::Head), 0.5);
        assert_eq!(
            cow_mob.equipment_in_slot(EquipmentSlot::Head).item().key,
            vanilla_items::IRON_HELMET.key
        );

        let cow_ageable = cow.as_ageable_mob().expect("cow is ageable");
        assert!(AgeableMob::is_baby(cow_ageable));
        assert_eq!(cow_ageable.get_age(), -24_000);
        assert_eq!(cow_ageable.forced_age(), 12);
        assert_eq!(cow_ageable.forced_age_timer(), 3);
    }

    #[test]
    fn single_conversion_reboards_the_root_passenger() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();
        let pig = add_pig(&world);
        let passenger = add_pig(&world);
        assert!(
            passenger.start_riding(&pig),
            "passenger should ride the pig"
        );
        assert_eq!(pig.first_passenger().map(|p| p.id()), Some(passenger.id()));

        let cow = pig
            .downcast_ref::<PigEntity>()
            .expect("pig is concrete")
            .convert_to(
                &vanilla_entities::COW,
                ConversionParams::single(false, false),
                None,
            )
            .expect("pig should convert to cow");

        assert_eq!(
            cow.first_passenger().map(|p| p.id()),
            Some(passenger.id()),
            "the root passenger reboards the converted mob"
        );
        assert!(!passenger.is_removed(), "the passenger survives conversion");
        assert!(pig.is_removed());
    }

    #[test]
    fn split_on_death_conversion_keeps_the_source_living() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();
        let pig = add_pig(&world);

        let cow = pig
            .downcast_ref::<PigEntity>()
            .expect("pig is concrete")
            .convert_to(
                &vanilla_entities::COW,
                ConversionParams {
                    conversion_type: ConversionType::SplitOnDeath,
                    keep_equipment: false,
                    preserve_can_pick_up_loot: false,
                },
                None,
            )
            .expect("split conversion should create the cow");

        assert_eq!(cow.entity_type(), &vanilla_entities::COW);
        assert!(world.get_entity_by_id(cow.id()).is_some());
        assert!(!pig.is_removed(), "split-on-death keeps the source living");
    }

    #[test]
    fn convert_to_fails_without_a_registered_factory() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();
        let pig = add_pig(&world);

        let converted = pig
            .downcast_ref::<PigEntity>()
            .expect("pig is concrete")
            .convert_to(
                &vanilla_entities::ZOMBIE,
                ConversionParams::single(true, true),
                None,
            );

        assert!(
            converted.is_none(),
            "no zombie factory is registered, so conversion cannot happen"
        );
        assert!(
            !pig.is_removed(),
            "failed conversions leave the source untouched"
        );
    }

    #[test]
    fn convert_to_refuses_hostile_types_on_peaceful_difficulty() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();

        // Peaceful: the registered hostile endermite factory must be refused.
        world.set_difficulty(Difficulty::Peaceful);
        let pig = add_pig(&world);
        let pig_mob = pig.as_mob().expect("pig is a mob");
        assert!(
            pig.downcast_ref::<PigEntity>()
                .expect("pig is concrete")
                .convert_to(
                    &vanilla_entities::ENDERMITE,
                    ConversionParams::single(true, true),
                    None,
                )
                .is_none(),
            "hostile conversions are refused on peaceful"
        );
        assert!(
            !pig_mob.is_removed(),
            "refused conversions leave the source untouched"
        );

        // Control: the same conversion is permitted off peaceful.
        world.set_difficulty(Difficulty::Normal);
        let other_pig = add_pig(&world);
        assert!(
            other_pig
                .downcast_ref::<PigEntity>()
                .expect("pig is concrete")
                .convert_to(
                    &vanilla_entities::ENDERMITE,
                    ConversionParams::single(true, true),
                    None,
                )
                .is_some(),
            "hostile conversions are allowed on non-peaceful difficulties"
        );
    }

    #[test]
    fn conversion_skips_equipment_when_not_requested() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();
        let pig = add_pig(&world);
        let pig_mob = pig.as_mob().expect("pig is a mob");
        pig_mob.living_base().equipment().lock().set(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::IRON_HELMET),
        );

        let cow = pig
            .downcast_ref::<PigEntity>()
            .expect("pig is concrete")
            .convert_to(
                &vanilla_entities::COW,
                ConversionParams::single(false, true),
                None,
            )
            .expect("pig should convert to cow");
        let cow_mob = cow.as_mob().expect("cow is a mob");

        assert!(
            cow_mob.equipment_in_slot(EquipmentSlot::Head).is_empty(),
            "equipment is only copied when keep_equipment is set"
        );
    }

    #[test]
    fn after_conversion_callback_receives_the_new_mob() {
        init_vanilla_registry();
        init_entities();
        let world = conversion_world();
        let pig = add_pig(&world);
        let finalized = Cell::new(false);
        let after_conversion = |mob: &dyn Mob| {
            assert_eq!(mob.entity_type(), &vanilla_entities::COW);
            finalized.set(true);
        };

        let cow = pig
            .downcast_ref::<PigEntity>()
            .expect("pig is concrete")
            .convert_to(
                &vanilla_entities::COW,
                ConversionParams::single(false, false),
                Some(&after_conversion),
            )
            .expect("pig should convert to cow");

        assert!(
            finalized.get(),
            "the after-conversion callback runs on the new mob"
        );
        assert_eq!(cow.entity_type(), &vanilla_entities::COW);
    }
}
