use std::sync::Arc;

use glam::DVec3;
use steel_registry::item_stack::ItemStack;
use steel_registry::{init_vanilla_registry, vanilla_damage_types, vanilla_items};
use steel_utils::types::InteractionHand;

use crate::behavior::init_behaviors;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, LivingEntity, SharedEntity};
use crate::player::Player;
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
use crate::world::World;
use steel_utils::ChunkPos;

/// Shield block delay is 0.25 seconds.
const SHIELD_BLOCK_DELAY_TICKS: usize = 5;

fn player_at(world: &Arc<World>, id: i32, position: DVec3, yaw: f32) -> Arc<Player> {
    let player = TestPlayerBuilder::new(Arc::clone(world), format!("Player{id}"), id).build();
    player.set_client_loaded(true);
    player.snap_to(position, yaw, 0.0);
    player.set_y_head_rot(yaw);
    player
}

/// Defender facing +Z at the origin, raising a shield in its off hand.
fn raise_shield(world: &Arc<World>) -> Arc<Player> {
    let defender = player_at(world, 1, DVec3::new(0.5, 64.0, 0.5), 0.0);
    defender
        .inventory
        .lock()
        .set_offhand_item(ItemStack::new(&vanilla_items::SHIELD));
    defender.start_using_item(InteractionHand::OffHand);
    defender
}

fn wait_for_block_delay(player: &Player) {
    for _ in 0..SHIELD_BLOCK_DELAY_TICKS {
        player.living_base().decrement_active_item_use();
    }
}

fn melee(attacker: &Arc<Player>) -> DamageSource {
    let attacker: SharedEntity = Arc::<Player>::clone(attacker);
    DamageSource::direct(&vanilla_damage_types::PLAYER_ATTACK, attacker)
}

fn offhand(player: &Player) -> ItemStack {
    player
        .inventory
        .lock()
        .get_item_in_hand(InteractionHand::OffHand)
        .clone()
}

#[test]
fn shield_blocks_frontal_hits_only_after_block_delay() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("shield_blocks_frontal_hits_only_after_block_delay");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let defender = raise_shield(world);
    let front = player_at(world, 2, DVec3::new(0.5, 64.0, 2.5), 180.0);

    defender.living_base().decrement_active_item_use();
    assert!(!defender.is_blocking());
    wait_for_block_delay(&defender);
    assert!(defender.is_blocking());

    assert!(!defender.hurt(world, &melee(&front), 4.0));
    assert_eq!(defender.get_health(), 20.0);
    // Item damage function: floor(1 + 1 * 4).
    assert_eq!(offhand(&defender).get_damage_value(), 5);
    assert!(defender.is_using_item());
}

#[test]
fn shield_does_not_block_hits_from_behind() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("shield_does_not_block_hits_from_behind");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let defender = raise_shield(world);
    let behind = player_at(world, 2, DVec3::new(0.5, 64.0, -1.5), 0.0);
    wait_for_block_delay(&defender);

    assert!(defender.hurt(world, &melee(&behind), 4.0));
    assert_eq!(defender.get_health(), 16.0);
    assert_eq!(offhand(&defender).get_damage_value(), 0);
}

#[test]
fn axe_hit_disables_raised_shield() {
    init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("axe_hit_disables_raised_shield");
    let world = &fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let defender = raise_shield(world);
    let front = player_at(world, 2, DVec3::new(0.5, 64.0, 2.5), 180.0);
    front
        .inventory
        .lock()
        .set_selected_item(ItemStack::new(&vanilla_items::IRON_AXE));
    wait_for_block_delay(&defender);

    assert!(!defender.hurt(world, &melee(&front), 4.0));

    assert!(!defender.is_using_item());
    assert!(defender.is_item_on_cooldown(&offhand(&defender)));
}
