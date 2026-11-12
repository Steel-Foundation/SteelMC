use std::sync::Arc;

use steel_registry::vanilla_entities;
use steel_utils::types::InteractionHand;

use super::*;

use crate::test_support::load_additional;
use crate::test_support::{TestPlayerBuilder, fresh_test_world, tick_test_world};

const POSITION: DVec3 = DVec3::new(-8.0, 128.0, 4.0);

fn interaction(world: &Arc<World>, id: i32) -> Arc<InteractionEntity> {
    InteractionEntity::new(
        &vanilla_entities::INTERACTION,
        id,
        POSITION,
        Arc::downgrade(world),
    )
}

#[test]
fn attacks_go_through_only_with_response() {
    let world_fixture = fresh_test_world("interaction_response");
    let world = &world_fixture.world;
    let player: SharedEntity = TestPlayerBuilder::new(Arc::clone(world), "Clicker", 9).build();
    let silent = interaction(world, 1);
    let responsive = interaction(world, 2);
    responsive.with_entity_data(|data| data.set_response(true));

    assert!(silent.skip_attack_interaction(&player));
    assert!(!responsive.skip_attack_interaction(&player));
}

#[test]
fn records_the_last_attack_and_interaction_with_game_time() {
    let world_fixture = fresh_test_world("interaction_records");
    let world = &world_fixture.world;
    let player = TestPlayerBuilder::new(Arc::clone(world), "Clicker", 9).build();
    let shared: SharedEntity = Arc::<Player>::clone(&player);
    let interaction = interaction(world, 1);
    for tick in 0..3 {
        tick_test_world(world, tick, true);
    }

    interaction.skip_attack_interaction(&shared);
    assert_eq!(
        interaction.last_attack(),
        Some(PlayerAction {
            player: player.uuid(),
            timestamp: 3
        })
    );
    assert_eq!(interaction.last_interaction(), None);

    tick_test_world(world, 3, true);
    interaction.interact(&player, InteractionHand::MainHand, POSITION);
    assert_eq!(
        interaction.last_interaction(),
        Some(PlayerAction {
            player: player.uuid(),
            timestamp: 4
        })
    );
}

#[test]
fn resizing_updates_the_box_including_on_load() {
    let world_fixture = fresh_test_world("interaction_resize");
    let interaction = interaction(&world_fixture.world, 1);
    let assert_size = |width: f32, height: f32| {
        let dimensions = interaction.base().dimensions();
        assert_eq!(dimensions.width.to_bits(), width.to_bits());
        assert_eq!(dimensions.height.to_bits(), height.to_bits());
        let bounding_box = interaction.base().bounding_box();
        assert_eq!(bounding_box.width().to_bits(), f64::from(width).to_bits());
        assert_eq!(bounding_box.height().to_bits(), f64::from(height).to_bits());
    };

    assert_size(DEFAULT_WIDTH, DEFAULT_HEIGHT);
    interaction.with_entity_data(|data| {
        data.set_width(0.5);
        data.set_height(0.25);
    });
    assert_size(0.5, 0.25);

    let mut nbt = NbtCompound::new();
    nbt.insert("width", 2.0_f32);
    load_additional(&*interaction, &nbt);
    assert_size(2.0, DEFAULT_HEIGHT);
}
