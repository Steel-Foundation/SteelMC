//! Helpers removed from the build with the entities that used them.
//! Restore next to their callers when those entities are ported.

// From src/entity/living_entity.rs (used by SheepEntity).
/// Runs vanilla `LivingEntity.dropFromShearingLootTable` for `loot_table`, returning the
/// drops resolved with the vanilla shearing loot params (origin, entity, tool).
pub(crate) fn shearing_loot_items_with_rng<R: rand::Rng, E: LivingEntity + ?Sized>(
    entity: &E,
    loot_table: LootTableRef,
    tool: &ItemStack,
    rng: &mut R,
) -> Vec<ItemStack> {
    let position = entity.position();
    let equipment = living_loot_equipment(entity);
    let mut context = LootContext::new(rng)
        .with_origin(position.x, position.y, position.z)
        .with_this_entity(entity_loot_ref(
            entity.as_entity_event_source(),
            Some(&equipment),
        ))
        .with_tool(tool);
    if let Some(level) = entity.level() {
        context = context.with_game_time(level.game_time());
    }
    loot_table.get_random_items(&mut context)
}

// From tests/support/mod.rs (used by projectile tests).
pub(crate) fn world_border_projectile_test_world() -> &'static Arc<World> {
    static WORLD: OnceLock<Arc<World>> = OnceLock::new();
    WORLD.get_or_init(|| {
        let world = create_static_test_world("test_world_border_projectile", Difficulty::Normal);
        let result = world.set_world_border_size(10.0);
        assert!(
            result.is_ok(),
            "test world border should resize: {result:?}"
        );
        world
    })
}
