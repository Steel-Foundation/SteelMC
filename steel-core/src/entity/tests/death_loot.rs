use crate::entity::entities::ItemEntity;
use crate::entity::next_entity_id;
use crate::player::{Player, ResetReason};
use crate::test_support::{
    TestEntity, TestPlayerBuilder, dropped_items, enchanted_item, with_enchantment,
};

use super::*;

const PIG_POSITION: DVec3 = DVec3::new(8.5, 64.0, 8.5);

fn death_loot_world(key: &'static str) -> Arc<World> {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world(key);
    insert_ready_full_chunk(&world, ChunkPos::from_entity_pos(PIG_POSITION));
    world
}

fn player_holding(world: &Arc<World>, weapon: ItemStack) -> Arc<Player> {
    let player =
        TestPlayerBuilder::new(Arc::clone(world), "PigKiller".to_owned(), next_entity_id()).build();
    player
        .inventory
        .lock()
        .set_item_in_hand(InteractionHand::MainHand, weapon);
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    player
}

fn kill_pig(world: &Arc<World>, loot_seed: i64, source: &DamageSource) {
    let pig = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        next_entity_id(),
        PIG_POSITION,
        Arc::downgrade(world),
    ));
    pig.set_death_loot_table_seed(loot_seed);
    world
        .try_add_entity(Arc::clone(&pig) as SharedEntity)
        .expect("pig should attach to the loaded chunk");
    pig.die(source);
}

fn dropped_item_stacks(world: &World) -> Vec<ItemStack> {
    dropped_items(world, PIG_POSITION)
        .iter()
        .filter_map(|entity| {
            entity
                .downcast_ref::<ItemEntity>()
                .map(ItemEntity::get_item)
        })
        .collect()
}

#[test]
fn melee_kill_with_fire_aspect_drops_cooked_porkchop() {
    let world = death_loot_world("death_loot_melee_fire_aspect");
    let player = player_holding(
        &world,
        enchanted_item(
            &vanilla_items::DIAMOND_SWORD,
            Identifier::vanilla_static("fire_aspect"),
            1,
        ),
    );
    let source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
        .with_causing_entity(player.id())
        .with_direct_entity(player.id());

    kill_pig(&world, 1, &source);

    let drops = dropped_item_stacks(&world);
    assert_eq!(drops.len(), 1);
    assert!(drops[0].is(&vanilla_items::COOKED_PORKCHOP));
}

#[test]
fn arrow_kill_smelts_by_direct_attacker_and_loots_by_attacker() {
    const KILLS: i64 = 20;

    let world = death_loot_world("death_loot_arrow_attackers");
    let weapon = with_enchantment(
        enchanted_item(
            &vanilla_items::DIAMOND_SWORD,
            Identifier::vanilla_static("fire_aspect"),
            1,
        ),
        Identifier::vanilla_static("looting"),
        3,
    );
    let player = player_holding(&world, weapon);
    let arrow_id = next_entity_id();
    world
        .try_add_entity(TestEntity::shared(
            arrow_id,
            PIG_POSITION,
            Arc::downgrade(&world),
            &vanilla_entities::ARROW,
        ))
        .expect("arrow should attach to the loaded chunk");
    let source = DamageSource::environment(&vanilla_damage_types::ARROW)
        .with_causing_entity(player.id())
        .with_direct_entity(arrow_id);

    for loot_seed in 1..=KILLS {
        kill_pig(&world, loot_seed, &source);
    }

    let drops = dropped_item_stacks(&world);
    assert_eq!(drops.len(), KILLS as usize);
    assert!(drops.iter().all(|drop| drop.is(&vanilla_items::PORKCHOP)));
    assert!(drops.iter().any(|drop| drop.count > 3));
}
