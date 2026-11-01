use super::{fixture::Fixture, helpers};
use glam::DVec3;
use std::error::Error;
use steel_core::entity::{Entity, entities::objects::items::ItemEntity};
use steel_registry::REGISTRY;
use steel_utils::{BlockPos, Downcast as _, WorldAabb};

pub(super) fn check(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let _runtime = fixture.runtime.enter();
    let pos = BlockPos::new(8, 65, 8);
    let chair = helpers::place(fixture, "chair", pos)?;
    helpers::interact(fixture, pos);
    let seat = fixture
        .player
        .vehicle()
        .ok_or("chair did not mount player")?;
    assert!(!seat.is_persistent());
    assert!(seat.nbt_for_passenger_save().is_none());
    assert!(seat.has_passenger(fixture.player.as_ref()));
    assert_eq!(fixture.player.position().x, 8.5);
    helpers::interact(fixture, pos);
    assert_eq!(fixture.player.vehicle().ok_or("lost seat")?.id(), seat.id());
    fixture.player.stop_riding();
    assert!(
        fixture.player.position().y >= 65.0,
        "dismount must leave the player above the floor"
    );
    // Save immediately, before its next tick removes the now-empty seat.
    let saved = helpers::saved(fixture)?;
    assert!(
        saved.pending_entities.is_empty(),
        "temporary seats must never enter chunk persistence"
    );
    fixture.world.tick_game(1, true);
    assert!(seat.is_removed());
    fixture
        .player
        .try_set_position(DVec3::new(6.5, 65.0, 8.5))?;
    helpers::interact(fixture, pos);
    let second = fixture.player.vehicle().ok_or("chair cannot be reused")?;
    assert_ne!(second.id(), seat.id());
    assert!(fixture.world.destroy_block(pos, true));
    fixture.world.tick_game(2, true);
    assert!(second.is_removed());
    assert!(fixture.player.vehicle().is_none());
    let item = REGISTRY.items.by_block(chair);
    let drops = fixture.world.get_entities_in_aabb(&WorldAabb::of_size(
        DVec3::new(8.5, 65.5, 8.5),
        4.0,
        4.0,
        4.0,
    ));
    assert_eq!(
        drops
            .iter()
            .filter_map(|entity| entity.downcast_ref::<ItemEntity>())
            .map(ItemEntity::get_item)
            .filter(|stack| stack.is(item))
            .map(|stack| stack.count())
            .sum::<i32>(),
        1
    );

    Ok(())
}
