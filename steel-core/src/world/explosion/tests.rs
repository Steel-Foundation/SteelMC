use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_rules::{BLOCK_EXPLOSION_DROP_DECAY, MOB_GRIEFING};
use steel_registry::{
    init_vanilla_registry, vanilla_blocks, vanilla_damage_types, vanilla_entities, vanilla_items,
};
use steel_utils::geometry::WorldAabb;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, ChunkPos, Downcast as _};

use super::{BlockInteraction, Explosion, ExplosionInteraction, SimpleExplosionDamageCalculator};
use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::entity::damage::DamageSource;
use crate::entity::entities::ItemEntity;
use crate::entity::{ENTITIES, SharedEntity, init_entities, next_entity_id};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
use crate::world::World;

/// The layer every terrain test builds its floor on.
const FLOOR_Y: i32 = 79;
/// Somewhere central in the loaded chunks, well clear of the world floor.
const CENTER: DVec3 = DVec3::new(8.5, 80.0, 8.5);

fn explosion_test_world(key: &'static str) -> Arc<World> {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    init_entities();
    let world = fresh_test_world(key);
    for x in -1..=1 {
        for z in -1..=1 {
            insert_ready_full_chunk(&world, ChunkPos::new(x, z));
        }
    }
    world
}

fn explosion_at(
    world: &Arc<World>,
    center: DVec3,
    radius: f32,
    interaction: BlockInteraction,
) -> Explosion {
    Explosion::new(world, None, None, None, center, radius, false, interaction)
}

/// Spawns an entity into the world, the way `/summon` does.
fn add_entity(world: &Arc<World>, entity_type: EntityTypeRef, position: DVec3) -> SharedEntity {
    let entity = ENTITIES
        .create(
            entity_type,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        )
        .expect("the generated factory should build the entity");
    world
        .try_add_entity(Arc::clone(&entity))
        .expect("the entity should be added");
    entity
}

fn health(entity: &SharedEntity) -> f32 {
    entity
        .as_living_entity()
        .expect("the target is a living entity")
        .get_health()
}

fn fill_layer(world: &Arc<World>, y: i32, block: BlockRef) {
    for x in 0..16 {
        for z in 0..16 {
            world.set_block(
                BlockPos::new(x, y, z),
                block.default_state(),
                UpdateFlags::UPDATE_NONE,
            );
        }
    }
}

#[test]
fn blast_proof_blocks_survive_a_direct_hit() {
    let world = explosion_test_world("explosion_bedrock");
    fill_layer(&world, FLOOR_Y, &vanilla_blocks::BEDROCK);

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Destroy).explode();

    // Bedrock drains a ray of far more power than it can carry, so it is never marked
    // for destruction however close the blast is.
    assert!(
        !world.get_block_state(BlockPos::new(8, FLOOR_Y, 8)).is_air(),
        "bedrock was broken"
    );
    assert!(
        dropped_items(&world).is_empty(),
        "bedrock dropped something"
    );
}

#[test]
fn a_blast_in_open_air_sees_all_of_an_entity() {
    let world = explosion_test_world("explosion_exposure_open");
    let pig = add_entity(&world, &vanilla_entities::PIG, DVec3::new(8.5, 80.0, 10.5));

    let exposure = Explosion::seen_percent(&world, CENTER, pig.as_ref());

    assert!(
        (exposure - 1.0).abs() < 1.0e-6,
        "an unobstructed pig was only {exposure} exposed"
    );
}

#[test]
fn a_wall_hides_an_entity_from_the_blast() {
    let world = explosion_test_world("explosion_exposure_walled");
    let pig = add_entity(&world, &vanilla_entities::PIG, DVec3::new(8.5, 80.0, 11.5));
    // A slab of stone straight through the line of sight, tall and wide enough that no
    // sample ray can go round it.
    for x in 4..14 {
        for y in 78..84 {
            world.set_block(
                BlockPos::new(x, y, 10),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            );
        }
    }

    let exposure = Explosion::seen_percent(&world, CENTER, pig.as_ref());

    assert_eq!(exposure, 0.0, "a fully walled pig was {exposure} exposed");
}

#[test]
fn a_blast_hurts_and_shoves_a_nearby_entity() {
    let world = explosion_test_world("explosion_damage");
    let pig = add_entity(&world, &vanilla_entities::PIG, DVec3::new(10.5, 80.0, 8.5));
    let health_before = health(&pig);

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Keep).explode();

    assert!(health(&pig) < health_before, "the pig took no damage");
    // Pushed away from the center, which sits to its west.
    assert!(pig.velocity().x > 0.0, "the pig was not shoved outwards");
}

#[test]
fn a_blast_spares_an_entity_out_of_range() {
    let world = explosion_test_world("explosion_out_of_range");
    // Beyond `radius * 2`, which is where vanilla's falloff reaches zero.
    let pig = add_entity(
        &world,
        &vanilla_entities::PIG,
        DVec3::new(8.5, 80.0, 8.5 + 9.0),
    );
    let health_before = health(&pig);

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Keep).explode();

    assert_eq!(health(&pig), health_before);
}

#[test]
fn a_calculator_can_turn_entity_damage_off() {
    let world = explosion_test_world("explosion_no_entity_damage");
    let pig = add_entity(&world, &vanilla_entities::PIG, DVec3::new(10.5, 80.0, 8.5));
    let health_before = health(&pig);

    Explosion::new(
        &world,
        None,
        None,
        Some(Box::new(SimpleExplosionDamageCalculator::new(
            true, false, None, None,
        ))),
        CENTER,
        4.0,
        false,
        BlockInteraction::Keep,
    )
    .explode();

    assert_eq!(health(&pig), health_before);
}

#[test]
fn a_blast_with_no_source_reports_as_environmental() {
    let world = explosion_test_world("explosion_damage_source");
    let explosion = explosion_at(&world, CENTER, 4.0, BlockInteraction::Keep);

    // Only a blast traced back to a player reports as `PLAYER_EXPLOSION`, which is what
    // puts a name in the death message.
    assert_eq!(
        explosion.damage_source().damage_type.message_id,
        vanilla_damage_types::EXPLOSION.message_id
    );
    assert_eq!(explosion.damage_source().source_position, Some(CENTER));
}

#[test]
fn only_a_block_breaking_blast_counts_as_large() {
    let world = explosion_test_world("explosion_is_small");

    assert!(explosion_at(&world, CENTER, 1.0, BlockInteraction::Destroy).is_small());
    assert!(!explosion_at(&world, CENTER, 4.0, BlockInteraction::Destroy).is_small());
    // A big blast that leaves blocks alone still uses the small effect.
    assert!(explosion_at(&world, CENTER, 4.0, BlockInteraction::Keep).is_small());
}

#[test]
fn block_interaction_reports_what_it_touches() {
    assert!(!BlockInteraction::Keep.interacts_with_blocks());
    assert!(BlockInteraction::TriggerBlock.interacts_with_blocks());
    assert!(!BlockInteraction::TriggerBlock.affects_blocklike_entities());
    assert!(BlockInteraction::DestroyWithDecay.affects_blocklike_entities());
}

/// A world whose only terrain is a stone floor under the blast.
fn stone_floor_world(key: &'static str) -> Arc<World> {
    let world = explosion_test_world(key);
    fill_layer(&world, FLOOR_Y, &vanilla_blocks::STONE);
    world
}

/// Blows up a fresh stone floor and totals what it dropped.
fn total_dropped(key: &'static str, interaction: BlockInteraction) -> i32 {
    let world = stone_floor_world(key);
    explosion_at(&world, CENTER, 4.0, interaction).explode();
    dropped_items(&world).iter().map(ItemStack::count).sum()
}

/// Counts the item entities the blast dropped anywhere near it.
fn dropped_items(world: &Arc<World>) -> Vec<ItemStack> {
    let area = WorldAabb::new(-32.0, 32.0, -32.0, 48.0, 128.0, 48.0);
    world
        .get_entities_in_aabb(&area)
        .iter()
        .filter_map(|entity| entity.as_ref().downcast_ref::<ItemEntity>())
        .map(ItemEntity::get_item)
        .collect()
}

#[test]
fn a_destroying_blast_clears_blocks_and_drops_them() {
    let world = stone_floor_world("explosion_destroy");

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Destroy).explode();

    assert!(
        world.get_block_state(BlockPos::new(8, FLOOR_Y, 8)).is_air(),
        "the block under the blast survived"
    );
    assert!(!dropped_items(&world).is_empty(), "nothing dropped");
}

#[test]
fn a_keep_blast_leaves_every_block_standing() {
    let world = stone_floor_world("explosion_keep");

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Keep).explode();

    assert!(
        !world.get_block_state(BlockPos::new(8, FLOOR_Y, 8)).is_air(),
        "a Keep blast broke a block"
    );
    assert!(
        dropped_items(&world).is_empty(),
        "a Keep blast dropped items"
    );
}

#[test]
fn a_trigger_blast_breaks_nothing_but_still_triggers() {
    let world = stone_floor_world("explosion_trigger");
    let explosion = explosion_at(&world, CENTER, 4.0, BlockInteraction::TriggerBlock);

    assert!(explosion.can_trigger_blocks());
    // Only a `TriggerBlock` blast reports this; the others let blocks be broken instead.
    assert!(!explosion_at(&world, CENTER, 4.0, BlockInteraction::Destroy).can_trigger_blocks());

    explosion_at(&world, CENTER, 4.0, BlockInteraction::TriggerBlock).explode();
    assert!(
        !world.get_block_state(BlockPos::new(8, FLOOR_Y, 8)).is_air(),
        "a TriggerBlock blast broke a block"
    );
}

#[test]
fn decay_costs_a_blast_most_of_its_drops() {
    // Both blasts break a comparable amount of stone; only the decaying one runs the
    // `explosion_decay` loot function, which gives each item a 1-in-radius survival
    // roll. This is the test that proves that wire is actually connected.
    //
    // Summed over several runs because a single blast's crater is jittered; the gap
    // is large (roughly a quarter of the drops survive) but not deterministic.
    const RUNS: i32 = 8;
    let mut plain = 0;
    let mut decayed = 0;
    for _ in 0..RUNS {
        plain += total_dropped("explosion_decay_plain", BlockInteraction::Destroy);
        decayed += total_dropped(
            "explosion_decay_decayed",
            BlockInteraction::DestroyWithDecay,
        );
    }

    assert!(
        decayed < plain,
        "decay dropped {decayed} items against {plain} without it"
    );
}

#[test]
fn merged_drops_stay_within_the_vanilla_cap() {
    let world = stone_floor_world("explosion_stack_cap");

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Destroy).explode();

    // Vanilla merges exploded drops but caps each stack at 16, well under stone's 64.
    for stack in dropped_items(&world) {
        assert!(
            stack.count() <= 16,
            "a merged drop held {} items",
            stack.count()
        );
    }
}

#[test]
fn a_fiery_blast_only_lights_solid_topped_air() {
    let world = stone_floor_world("explosion_fire");

    Explosion::new(
        &world,
        None,
        None,
        None,
        CENTER,
        4.0,
        true,
        BlockInteraction::Keep,
    )
    .explode();

    // `Keep` breaks nothing, so the stone floor stays and any fire must sit on it.
    for x in 0..16 {
        for z in 0..16 {
            let pos = BlockPos::new(x, 80, z);
            if world.get_block_state(pos).get_block() == &vanilla_blocks::FIRE {
                assert!(
                    world.get_block_state(pos.below()).is_solid_render(),
                    "fire at {pos:?} had nothing solid beneath it"
                );
            }
        }
    }
}

#[test]
fn an_exploded_container_spills_its_contents() {
    let world = explosion_test_world("explosion_container");
    let pos = BlockPos::new(8, 80, 8);
    assert!(world.set_block(
        pos,
        vanilla_blocks::BARREL.default_state(),
        UpdateFlags::UPDATE_ALL
    ));

    let block_entity = world
        .get_block_entity(pos)
        .expect("a barrel has a block entity");
    let container_ref =
        ContainerRef::from_block_entity(block_entity).expect("a barrel is a container");
    {
        let mut guard = ContainerLockGuard::lock_all(&[&container_ref]);
        guard
            .get_mut(container_ref.container_id())
            .expect("the barrel is locked")
            .set_item(0, ItemStack::with_count(&vanilla_items::DIAMOND, 3));
    }

    explosion_at(&world, CENTER, 4.0, BlockInteraction::Destroy).explode();

    // Vanilla spills a container from `BlockEntity.preRemoveSideEffects`, which Steel
    // has no equivalent of yet, so the explosion asks for it explicitly. Without that
    // the diamonds would vanish with the barrel.
    let diamonds: i32 = dropped_items(&world)
        .iter()
        .filter(|stack| stack.item() == &*vanilla_items::DIAMOND)
        .map(ItemStack::count)
        .sum();
    assert_eq!(diamonds, 3, "the barrel's contents did not drop");
}

#[test]
fn a_drop_decay_rule_picks_between_the_two_destroy_modes() {
    let world = explosion_test_world("explosion_decay_rule");

    assert_eq!(
        ExplosionInteraction::Block.resolve(&world),
        BlockInteraction::DestroyWithDecay
    );

    world.set_game_rule(&BLOCK_EXPLOSION_DROP_DECAY, false);
    assert_eq!(
        ExplosionInteraction::Block.resolve(&world),
        BlockInteraction::Destroy
    );
}

#[test]
fn mob_griefing_suppresses_a_mob_blast_entirely() {
    let world = explosion_test_world("explosion_mob_griefing");

    assert_eq!(
        ExplosionInteraction::Mob.resolve(&world),
        BlockInteraction::DestroyWithDecay
    );

    // A mob's blast is the only one the gamerule can switch off outright.
    world.set_game_rule(&MOB_GRIEFING, false);
    assert_eq!(
        ExplosionInteraction::Mob.resolve(&world),
        BlockInteraction::Keep
    );
    assert_eq!(
        ExplosionInteraction::Block.resolve(&world),
        BlockInteraction::DestroyWithDecay
    );
}

#[test]
fn the_remaining_interactions_ignore_the_gamerules() {
    let world = explosion_test_world("explosion_interaction_fixed");
    world.set_game_rule(&MOB_GRIEFING, false);

    assert_eq!(
        ExplosionInteraction::None.resolve(&world),
        BlockInteraction::Keep
    );
    assert_eq!(
        ExplosionInteraction::Trigger.resolve(&world),
        BlockInteraction::TriggerBlock
    );
}

#[test]
fn a_struck_crystal_is_destroyed_and_detonates() {
    let world = stone_floor_world("explosion_crystal");
    let crystal = add_entity(&world, &vanilla_entities::END_CRYSTAL, CENTER);

    assert!(crystal.hurt(
        &world,
        &DamageSource::environment(&vanilla_damage_types::GENERIC),
        1.0
    ));

    assert!(crystal.is_removed(), "the crystal survived being hit");
    assert!(
        world.get_block_state(BlockPos::new(8, FLOOR_Y, 8)).is_air(),
        "the crystal did not crater the floor"
    );
}

#[test]
fn a_crystal_caught_in_a_blast_does_not_chain() {
    let world = stone_floor_world("explosion_crystal_chain");
    let crystal = add_entity(&world, &vanilla_entities::END_CRYSTAL, CENTER);

    // An explosion-sourced hit removes the crystal without setting off another blast,
    // which is what stops a ring of them detonating without end.
    assert!(crystal.hurt(
        &world,
        &DamageSource::environment(&vanilla_damage_types::EXPLOSION),
        1.0
    ));

    assert!(crystal.is_removed());
    assert!(
        !world.get_block_state(BlockPos::new(8, FLOOR_Y, 8)).is_air(),
        "a chained crystal cratered the floor"
    );
}

#[test]
fn an_ordinary_attacker_still_breaks_a_crystal() {
    let world = stone_floor_world("explosion_crystal_attacker");
    let crystal = add_entity(&world, &vanilla_entities::END_CRYSTAL, CENTER);
    let pig = add_entity(&world, &vanilla_entities::PIG, DVec3::new(12.5, 80.0, 8.5));

    // The crystal refuses damage from an ender dragon specifically. That branch cannot
    // be exercised here, because the dragon's entity type has no Rust implementation on this
    // branch, so the factory cannot build one, but this proves the guard does not
    // over-trigger and reject every attacker.
    let source =
        DamageSource::environment(&vanilla_damage_types::MOB_ATTACK).with_causing_entity(pig.id());

    assert!(crystal.hurt(&world, &source, 10.0));
    assert!(crystal.is_removed());
}
