use std::io::Cursor;

use glam::DVec3;
use simdnbt::borrow::read_compound;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{vanilla_damage_types, vanilla_entities};
use steel_utils::types::UpdateFlags;
use steel_utils::{ChunkPos, WorldAabb};

use super::*;
use crate::entity::entities::objects::vehicles::abstract_minecart::Parts as _;
use crate::entity::entities::objects::vehicles::test_support::test_world;
use crate::entity::entities::objects::vehicles::vehicle_entity::Parts as _;
use crate::entity::next_entity_id;

const CART_BLOCK_POS: BlockPos = BlockPos::new(8, 64, 8);

fn spawned_tnt_minecart(world: &Arc<World>) -> Arc<TntMinecartEntity> {
    let cart = TntMinecartEntity::new(
        &vanilla_entities::TNT_MINECART,
        next_entity_id(),
        CART_BLOCK_POS.get_bottom_center(),
        Arc::downgrade(world),
    );
    world
        .try_add_entity(Arc::<TntMinecartEntity>::clone(&cart))
        .expect("test TNT minecart should enter the loaded chunk");
    cart
}

fn set_block(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    let flags = UpdateFlags::UPDATE_NONE
        | UpdateFlags::UPDATE_KNOWN_SHAPE
        | UpdateFlags::UPDATE_SKIP_ON_PLACE;
    assert!(world.set_block(pos, state, flags));
}

fn dropped_items(world: &World, position: DVec3) -> usize {
    world
        .get_entities_in_aabb_matching(&WorldAabb::of_size(position, 4.0, 4.0, 4.0), |entity| {
            entity.entity_type() == &vanilla_entities::ITEM
        })
        .len()
}

#[test]
fn only_powered_activator_rails_prime_the_fuse() {
    let fixture = test_world("tnt_minecart_activation", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let cart = spawned_tnt_minecart(world);

    cart.abstract_minecart()
        .activate_minecart(world, CART_BLOCK_POS, false);
    assert!(!cart.is_primed());

    cart.abstract_minecart()
        .activate_minecart(world, CART_BLOCK_POS, true);
    assert_eq!(cart.fuse(), Some(PrimedTntEntity::DEFAULT_FUSE_TIME));

    // A lit fuse is not restarted by later activator rails.
    cart.state.lock().fuse = Some(5);
    cart.abstract_minecart()
        .activate_minecart(world, CART_BLOCK_POS, true);
    assert_eq!(cart.fuse(), Some(5));
}

#[test]
fn burnt_out_fuse_explodes_but_spares_rails_and_their_support() {
    let fixture = test_world("tnt_minecart_rail_protection", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let support_pos = CART_BLOCK_POS.below();
    let glass_pos = CART_BLOCK_POS.east();
    set_block(world, support_pos, vanilla_blocks::STONE.default_state());
    set_block(world, CART_BLOCK_POS, vanilla_blocks::RAIL.default_state());
    set_block(world, glass_pos, vanilla_blocks::GLASS.default_state());
    let cart = spawned_tnt_minecart(world);

    cart.prime_fuse(None);
    cart.state.lock().fuse = Some(0);
    cart.tick();

    assert!(cart.is_removed());
    assert_eq!(
        world.get_block_state(CART_BLOCK_POS).get_block(),
        &vanilla_blocks::RAIL
    );
    assert_eq!(
        world.get_block_state(support_pos).get_block(),
        &vanilla_blocks::STONE
    );
    assert!(world.get_block_state(glass_pos).is_air());
}

#[test]
fn slow_non_igniting_destruction_drops_the_cart_item() {
    let fixture = test_world("tnt_minecart_plain_destroy", ChunkPos::new(0, 0));
    let world = &fixture.world;
    assert!(world.set_game_rule(&ENTITY_DROPS, true));
    let cart = spawned_tnt_minecart(world);

    cart.destroy(
        world,
        &DamageSource::environment(&vanilla_damage_types::GENERIC),
    );

    assert!(!cart.is_primed());
    assert!(cart.is_removed());
    assert_eq!(dropped_items(world, cart.position()), 1);
}

#[test]
fn fire_damage_destroys_into_a_short_random_fuse() {
    let fixture = test_world("tnt_minecart_fire_destroy", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let cart = spawned_tnt_minecart(world);

    // Fire destroys the cart regardless of accumulated damage.
    assert!(cart.hurt(
        world,
        &DamageSource::environment(&vanilla_damage_types::IN_FIRE),
        0.0,
    ));

    assert!(!cart.is_removed());
    let max_fuse = 2 * (DESTROYED_FUSE_RANDOM_BOUND - 1);
    assert!(
        cart.fuse()
            .is_some_and(|fuse| (0..=max_fuse).contains(&fuse))
    );
    assert!(cart.state.lock().ignition_source.is_some());
}

#[test]
fn ignition_cause_keeps_attribution_after_removal() {
    let fixture = test_world("tnt_minecart_removed_cause", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let cart = spawned_tnt_minecart(world);
    let cause: SharedEntity = spawned_tnt_minecart(world);

    cart.prime_fuse(Some(&DamageSource::explosion(
        Some(Arc::clone(&cause)),
        Some(Arc::clone(&cause)),
    )));
    // A creeper is discarded by its own explosion before the cart's fuse burns out.
    cause.set_removed(RemovalReason::Discarded);

    let source = cart
        .ignition_damage_source()
        .expect("ignited cart should have an ignition source");
    assert!(
        source
            .causing_entity()
            .is_some_and(|entity| Arc::ptr_eq(entity, &cause))
    );
}

#[test]
fn disabled_tnt_still_burns_a_destroyed_fuse_then_discards_without_exploding() {
    let fixture = test_world("tnt_minecart_disabled_tnt", ChunkPos::new(0, 0));
    let world = &fixture.world;
    assert!(world.set_game_rule(&TNT_EXPLODES, false));
    let glass_pos = CART_BLOCK_POS.east();
    set_block(world, glass_pos, vanilla_blocks::GLASS.default_state());
    let cart = spawned_tnt_minecart(world);

    cart.abstract_minecart()
        .activate_minecart(world, CART_BLOCK_POS, true);
    assert!(!cart.is_primed());

    cart.destroy(
        world,
        &DamageSource::environment(&vanilla_damage_types::IN_FIRE),
    );
    assert!(cart.is_primed());

    cart.state.lock().fuse = Some(0);
    cart.tick();

    assert!(cart.is_removed());
    assert_eq!(
        world.get_block_state(glass_pos).get_block(),
        &vanilla_blocks::GLASS
    );
}

#[test]
fn only_falls_of_three_blocks_or_more_explode() {
    let fixture = test_world("tnt_minecart_fall", ChunkPos::new(0, 0));
    let world = &fixture.world;
    let source = DamageSource::environment(&vanilla_damage_types::FALL);

    let short_fall = spawned_tnt_minecart(world);
    short_fall.cause_fall_damage(2.9, 1.0, &source);
    assert!(!short_fall.is_removed());

    let long_fall = spawned_tnt_minecart(world);
    long_fall.cause_fall_damage(MIN_EXPLOSIVE_FALL_DISTANCE, 1.0, &source);
    assert!(long_fall.is_removed());
}

#[test]
fn saved_fuse_and_explosion_values_round_trip_with_vanilla_clamping() {
    let fixture = test_world("tnt_minecart_nbt", ChunkPos::new(0, 0));
    let cart = spawned_tnt_minecart(&fixture.world);

    let mut defaults = NbtCompound::new();
    cart.save_additional(&mut defaults);
    assert_eq!(defaults.int("fuse"), Some(UNPRIMED_FUSE));
    assert!(defaults.float("explosion_power").is_none());
    assert!(defaults.float("explosion_speed_factor").is_none());

    let mut saved = NbtCompound::new();
    saved.insert("fuse", 12);
    saved.insert("explosion_power", 500.0_f32);
    saved.insert("explosion_speed_factor", -3.0_f32);
    let mut bytes = Vec::new();
    saved.write(&mut bytes);
    let borrowed =
        read_compound(&mut Cursor::new(bytes.as_slice())).expect("saved NBT should parse");
    cart.load_additional((&borrowed).into());

    let state = cart.state.lock();
    assert_eq!(state.fuse, Some(12));
    assert_eq!(state.explosion_power_base, MAX_SAVED_EXPLOSION_VALUE);
    assert_eq!(state.explosion_speed_factor, 0.0);
}
