use super::*;

use crate::chunk::chunk_holder::TickingReadiness;
use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
use crate::level_data::GameTimeSource;
use crate::test_support::create_test_world_with_time_source;
use steel_registry::packets::play::C_PLAYER_POSITION;
use steel_registry::vanilla_game_rules;
use steel_utils::{Identifier, types::Difficulty};

#[test]
fn uuid_pearl_impact_overworld_to_nether() {
    assert_pearl_impact(false, true, false);
}

#[test]
fn uuid_pearl_impact_nether_to_overworld() {
    assert_pearl_impact(true, false, false);
}

#[test]
fn same_dimension_uuid_pearl_impact() {
    assert_pearl_impact(false, false, false);
    assert_pearl_impact(true, true, false);
}

#[test]
fn cached_owner_pearl_impact_between_dimensions() {
    assert_pearl_impact(false, true, true);
    assert_pearl_impact(true, false, true);
}

fn assert_pearl_impact(owner_in_nether: bool, pearl_in_nether: bool, cached: bool) {
    init_behaviors();
    init_entities();
    let overworld_fixture = fresh_test_world_in_domain("pearl_impact", "overworld");
    let overworld = &overworld_fixture.world;
    let nether_fixture = create_test_world_with_time_source(
        Identifier::new_static("pearl_impact", "the_nether"),
        Difficulty::Normal,
        &vanilla_dimension_types::THE_NETHER,
        GameTimeSource::Derived(Arc::clone(&overworld.game_time)),
    );
    let nether = &nether_fixture.world;
    for world in [overworld, nether] {
        let chunk_pos = ChunkPos::new(0, 0);
        let holder = insert_ready_full_chunk(world, chunk_pos);
        holder.set_simulation_level(Some(ChunkTicketLevel::ENTITY_TICKING_CHUNK));
        holder.transition_ticking_readiness(TickingReadiness::EntityTicking);
        world.update_entity_chunk_visibility(chunk_pos, holder.entity_visibility());
        assert!(world.set_game_rule(&vanilla_game_rules::SPAWN_MOBS, false));
    }
    let owner_world = if owner_in_nether { nether } else { overworld };
    let pearl_world = if pearl_in_nether { nether } else { overworld };
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let domain = ResolvedDomainConfig {
            name: "pearl_impact".to_owned(),
            default_world: overworld.key.clone(),
            worlds: vec![overworld.key.clone(), nether.key.clone()],
        };
        let server = test_server_with_worlds(
            domain.name.clone(),
            slice::from_ref(&domain),
            &[Arc::clone(overworld), Arc::clone(nether)],
            PermissionSubjectIndex::new(),
        )
        .await
        .expect("test server");
        let (player, packets) =
            test_player_with_packets(&server, Arc::clone(owner_world), "Owner", 1);
        player.base().set_position_local(DVec3::new(8.5, 65.0, 8.5));
        assert!(server.online_players.insert(Arc::clone(&player)));
        assert!(owner_world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let _ = player.mark_joined_world();
        assert!(pearl_world.set_block(
            BlockPos::new(0, 64, 0),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let pearl = Arc::new(EnderPearlEntity::new(
            &vanilla_entities::ENDER_PEARL,
            2,
            DVec3::new(0.5, 65.25, 0.5),
            Arc::downgrade(pearl_world),
        ));
        if cached {
            let owner: SharedEntity = Arc::<Player>::clone(&player);
            pearl.set_owner_entity(Some(&owner));
        } else {
            pearl.set_owner_uuid(Some(player.uuid()));
        }
        pearl.set_velocity(DVec3::new(0.0, -1.0, 0.0));
        let shared_pearl: SharedEntity = Arc::<EnderPearlEntity>::clone(&pearl);
        pearl_world
            .try_add_entity(shared_pearl)
            .expect("register pearl");
        packets.lock().clear();
        // Tick the entity managers serially, isolating lookup from world scheduling (#729).
        overworld.entity_manager().tick_entities(1, true);
        nether.entity_manager().tick_entities(1, true);
        assert!(pearl.is_removed());
        assert!(Arc::ptr_eq(&player.get_world(), pearl_world));
        assert_eq!(player.position(), DVec3::new(0.5, 65.25, 0.5));
        assert_eq!(player.get_health(), 15.0);
        assert!(
            packets
                .lock()
                .iter()
                .any(|packet| packet_id(packet) == C_PLAYER_POSITION)
        );

        player.get_world().remove_player_for_world_change(&player);
        assert!(server.remove_online_player_sync(&player).is_some());
        for world in [overworld, nether] {
            world.chunk_map.stop_generation_refill_loop();
            world.chunk_map.task_tracker.close();
            world.chunk_map.task_tracker.wait().await;
        }
    });
}
