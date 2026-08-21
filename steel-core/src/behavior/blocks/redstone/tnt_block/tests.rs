use std::sync::Arc;

use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::init_vanilla_registry;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_rules::{MOB_GRIEFING, TNT_EXPLODES};
use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_items};
use steel_utils::types::InteractionHand;
use steel_utils::{ChunkPos, Direction, Downcast as _, WorldAabb};

use super::*;
use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
use crate::entity::Entity;
use crate::entity::entities::{EnderPearlEntity, FireworkRocketEntity, PigEntity, PrimedTntEntity};
use crate::player::ResetReason;
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::{ExplosionInteraction, ExplosionOptions};

const TEST_TNT_POS: BlockPos = BlockPos::new(8, 64, 8);
const ONE_BLOCK: f64 = 1.0;
const TWO_BLOCKS: f64 = 2.0;
const VANILLA_DEFAULT_FUSE_TIME: i32 = 80;
const VANILLA_MINIMUM_SHORT_FUSE: i32 = 10;
const VANILLA_SHORT_FUSE_RANDOM_RANGE: i32 = 20;
const BURNING_PROJECTILE_FIRE_TICKS: i32 = 20;
const STANDARD_TNT_EXPLOSION_POWER: f32 = 4.0;
const PEARL_SPEED_BLOCKS_PER_TICK: f64 = 1.5;
const OWNER_DISTANCE_BLOCKS: i32 = 4;

fn setup_world(key: &'static str) -> (Arc<World>, BlockPos) {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_TNT_POS));
    (world, TEST_TNT_POS)
}

fn primed_tnt_entities(world: &World, pos: BlockPos) -> Vec<SharedEntity> {
    let bounds = WorldAabb::new(
        f64::from(pos.x()),
        f64::from(pos.y()) - ONE_BLOCK,
        f64::from(pos.z()),
        f64::from(pos.x()) + ONE_BLOCK,
        f64::from(pos.y()) + TWO_BLOCKS,
        f64::from(pos.z()) + ONE_BLOCK,
    );
    world
        .get_entities_in_aabb(&bounds)
        .into_iter()
        .filter(|entity| entity.downcast_ref::<PrimedTntEntity>().is_some())
        .collect()
}

fn only_primed_tnt(world: &World, pos: BlockPos) -> SharedEntity {
    let mut entities = primed_tnt_entities(world, pos);
    assert_eq!(entities.len(), 1, "expected exactly one primed TNT");
    entities.pop().expect("the primed TNT count was checked")
}

fn block_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_center();
    DVec3::new(x, y, z)
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}

fn test_player(world: &Arc<World>, name: &'static str) -> Arc<Player> {
    TestPlayerBuilder::new(Arc::clone(world), name, next_entity_id()).build()
}

fn hit_result(pos: BlockPos) -> BlockHitResult {
    BlockHitResult {
        location: block_center(pos),
        direction: Direction::Up,
        block_pos: pos,
        miss: false,
        inside: false,
        world_border_hit: false,
    }
}

fn clip_hit_result(pos: BlockPos) -> ClipHitResult {
    let hit = hit_result(pos);
    ClipHitResult {
        location: hit.location,
        direction: hit.direction,
        block_pos: hit.block_pos,
        miss: hit.miss,
        inside: hit.inside,
        world_border_hit: hit.world_border_hit,
    }
}

#[test]
fn powered_placement_primes_and_removes_tnt() {
    let (world, pos) = setup_world("tnt_redstone_placement");
    assert!(world.set_block(
        pos.west(),
        vanilla_blocks::REDSTONE_BLOCK.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));

    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));

    assert!(world.get_block_state(pos).is_air());
    let primed = only_primed_tnt(&world, pos);
    assert_eq!(
        primed
            .downcast_ref::<PrimedTntEntity>()
            .map(PrimedTntEntity::fuse),
        Some(VANILLA_DEFAULT_FUSE_TIME)
    );
}

#[test]
fn neighbor_power_primes_an_existing_tnt_block() {
    let (world, pos) = setup_world("tnt_redstone_neighbor");
    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));

    assert!(world.set_block(
        pos.west(),
        vanilla_blocks::REDSTONE_BLOCK.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));

    assert!(world.get_block_state(pos).is_air());
    only_primed_tnt(&world, pos);
}

#[test]
fn unstable_survival_break_primes_tnt_without_a_player_owner() {
    let (world, pos) = setup_world("tnt_unstable_survival_break");
    let state = vanilla_blocks::TNT
        .default_state()
        .set_value(&BlockStateProperties::UNSTABLE, true);
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    let player = test_player(&world, "Breaker");

    let returned_state = BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .player_will_destroy(state, &world, pos, &player);

    assert_eq!(returned_state, state);
    let primed = only_primed_tnt(&world, pos);
    assert!(primed.explosion_indirect_source().is_none());
}

#[test]
fn flint_and_steel_primes_tnt_damages_item_and_tracks_owner() {
    let (world, pos) = setup_world("tnt_flint_and_steel");
    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let player = test_player(&world, "Igniter");
    player.base().set_position_local(block_bottom_center(pos));
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::FLINT_AND_STEEL));
    let initial_damage = player
        .inventory
        .lock()
        .get_selected_item()
        .get_damage_value();
    let mut inventory = InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);

    let result = BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .use_item_on(
            world.get_block_state(pos),
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit_result(pos),
            &mut inventory,
        );

    assert_eq!(result, InteractionResult::Success);
    assert!(world.get_block_state(pos).is_air());
    assert_eq!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .get_damage_value(),
        initial_damage + 1
    );
    let primed = only_primed_tnt(&world, pos);
    assert_eq!(
        primed.explosion_indirect_source().map(|owner| owner.id()),
        Some(player.id())
    );
}

#[test]
fn fire_charge_primes_tnt_and_consumes_one_item() {
    let (world, pos) = setup_world("tnt_fire_charge");
    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let player = test_player(&world, "Charger");
    let initial_charge_count = 2;
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::with_count(
            &vanilla_items::FIRE_CHARGE,
            initial_charge_count,
        ));
    let mut inventory = InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);

    let result = BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .use_item_on(
            world.get_block_state(pos),
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit_result(pos),
            &mut inventory,
        );

    assert_eq!(result, InteractionResult::Success);
    assert_eq!(
        player.inventory.lock().get_selected_item().count(),
        initial_charge_count - 1
    );
    only_primed_tnt(&world, pos);
}

#[test]
fn disabled_tnt_gamerule_preserves_block_and_ignition_item() {
    let (world, pos) = setup_world("tnt_disabled_gamerule");
    assert!(world.set_game_rule(&TNT_EXPLODES, false));
    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let player = test_player(&world, "Disabled");
    player
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::FLINT_AND_STEEL));
    let initial_damage = player
        .inventory
        .lock()
        .get_selected_item()
        .get_damage_value();
    let mut inventory = InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);

    let result = BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .use_item_on(
            world.get_block_state(pos),
            &world,
            pos,
            &player,
            InteractionHand::MainHand,
            &hit_result(pos),
            &mut inventory,
        );

    assert_eq!(result, InteractionResult::Pass);
    assert_eq!(world.get_block_state(pos).get_block(), &vanilla_blocks::TNT);
    assert_eq!(
        player
            .inventory
            .lock()
            .get_selected_item()
            .get_damage_value(),
        initial_damage
    );
    assert!(primed_tnt_entities(&world, pos).is_empty());
}

#[test]
fn explosion_replaces_tnt_with_a_short_fuse_entity() {
    let (world, pos) = setup_world("tnt_chain_reaction");
    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let owner = test_player(&world, "ChainOwner");
    let owner_pos = pos.offset(OWNER_DISTANCE_BLOCKS, 0, 0);
    owner
        .base()
        .set_position_local(block_bottom_center(owner_pos));
    assert!(world.add_player(Arc::clone(&owner), ResetReason::InitialJoin));
    let mut options = ExplosionOptions::new(
        block_center(pos),
        STANDARD_TNT_EXPLOSION_POWER,
        ExplosionInteraction::Tnt,
    );
    options.source = Some(owner.as_ref());

    world.explode(options);

    assert!(world.get_block_state(pos).is_air());
    let primed = only_primed_tnt(&world, pos);
    let fuse = primed
        .downcast_ref::<PrimedTntEntity>()
        .map(PrimedTntEntity::fuse)
        .expect("spawned entity should remain primed TNT");
    let valid_short_fuses =
        VANILLA_MINIMUM_SHORT_FUSE..VANILLA_MINIMUM_SHORT_FUSE + VANILLA_SHORT_FUSE_RANDOM_RANGE;
    assert!(valid_short_fuses.contains(&fuse));
    assert_eq!(
        primed.explosion_indirect_source().map(|source| source.id()),
        Some(owner.id())
    );
}

#[test]
fn fuse_tick_detonates_primed_tnt() {
    let (world, pos) = setup_world("primed_tnt_detonation");
    assert!(world.set_block(
        pos,
        vanilla_blocks::GLASS.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    let entity = Arc::new(PrimedTntEntity::new(
        &vanilla_entities::TNT,
        next_entity_id(),
        block_bottom_center(pos),
        Arc::downgrade(&world),
    ));
    let final_fuse_tick = 1;
    entity.set_fuse(final_fuse_tick);
    let shared: SharedEntity = entity.clone();
    world
        .try_add_entity(shared)
        .expect("primed TNT should enter the loaded test chunk");

    entity.tick();

    assert!(entity.is_removed());
    assert!(world.get_block_state(pos).is_air());
}

#[test]
fn burning_projectile_primes_tnt() {
    let (world, pos) = setup_world("tnt_burning_projectile");
    let state = vanilla_blocks::TNT.default_state();
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    let projectile = FireworkRocketEntity::new(
        &vanilla_entities::FIREWORK_ROCKET,
        next_entity_id(),
        block_center(pos),
        Arc::downgrade(&world),
    );
    projectile.set_remaining_fire_ticks(BURNING_PROJECTILE_FIRE_TICKS);
    let clip_hit = clip_hit_result(pos);

    BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .on_projectile_hit(state, &world, &clip_hit, &projectile);

    assert!(world.get_block_state(pos).is_air());
    only_primed_tnt(&world, pos);
}

#[test]
fn ender_pearl_moving_from_lava_primes_tnt_on_hit() {
    let (world, pos) = setup_world("tnt_burning_ender_pearl");
    assert!(world.set_block(
        pos.west(),
        vanilla_blocks::LAVA.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));
    assert!(world.set_block(
        pos,
        vanilla_blocks::TNT.default_state(),
        UpdateFlags::UPDATE_NONE,
    ));

    let pearl = Arc::new(EnderPearlEntity::new(
        &vanilla_entities::ENDER_PEARL,
        next_entity_id(),
        block_center(pos.west()),
        Arc::downgrade(&world),
    ));
    let entity: SharedEntity = pearl.clone();
    world
        .try_add_entity(entity)
        .expect("ender pearl should enter the loaded test chunk");
    pearl.set_velocity(DVec3::X * PEARL_SPEED_BLOCKS_PER_TICK);
    assert!(!pearl.is_on_fire());

    pearl.tick();

    assert!(pearl.is_removed());
    assert!(pearl.is_on_fire());
    assert!(world.get_block_state(pos).is_air());
    only_primed_tnt(&world, pos);
}

#[test]
fn burning_projectile_preserves_its_living_owner() {
    let (world, pos) = setup_world("tnt_burning_projectile_owner");
    let state = vanilla_blocks::TNT.default_state();
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    let player = test_player(&world, "Archer");
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let projectile = FireworkRocketEntity::new(
        &vanilla_entities::FIREWORK_ROCKET,
        next_entity_id(),
        block_center(pos),
        Arc::downgrade(&world),
    );
    projectile.set_owner_uuid(Some(player.uuid()));
    projectile.set_remaining_fire_ticks(BURNING_PROJECTILE_FIRE_TICKS);
    let clip_hit = clip_hit_result(pos);

    BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .on_projectile_hit(state, &world, &clip_hit, &projectile);

    let primed = only_primed_tnt(&world, pos);
    assert_eq!(
        primed.explosion_indirect_source().map(|owner| owner.id()),
        Some(player.id())
    );
}

#[test]
fn burning_mob_owned_projectile_respects_mob_griefing() {
    let (world, pos) = setup_world("tnt_projectile_mob_griefing");
    assert!(world.set_game_rule(&MOB_GRIEFING, false));
    let state = vanilla_blocks::TNT.default_state();
    assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
    let projectile = FireworkRocketEntity::new(
        &vanilla_entities::FIREWORK_ROCKET,
        next_entity_id(),
        block_center(pos),
        Arc::downgrade(&world),
    );
    let mob = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        block_bottom_center(pos),
        Arc::downgrade(&world),
    ));
    let shared_mob: SharedEntity = mob.clone();
    assert!(world.try_add_entity(shared_mob).is_ok());
    projectile.set_owner_uuid(Some(mob.uuid()));
    projectile.set_remaining_fire_ticks(BURNING_PROJECTILE_FIRE_TICKS);
    let clip_hit = clip_hit_result(pos);

    BLOCK_BEHAVIORS
        .get_behavior(&vanilla_blocks::TNT)
        .on_projectile_hit(state, &world, &clip_hit, &projectile);

    assert_eq!(world.get_block_state(pos), state);
    assert!(primed_tnt_entities(&world, pos).is_empty());
}
