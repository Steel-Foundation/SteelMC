//! Monster spawn rule helpers not yet needed by a ported monster. Port them
//! into `src/entity/monster/` (as free functions) with the first monster that
//! uses them; their tests are in `tests/entity/monster/tests.rs`.

    /// Checks dimension light limits using the caller's seeded RNG.
    fn is_dark_enough_to_spawn(level: &World, pos: BlockPos, random: &mut impl Random) -> bool
    where
        Self: Sized,
    {
        if i32::from(level.light_value_at(LightLayer::Sky, pos))
            > random.next_i32_bounded(SKY_LIGHT_SPAWN_ROLL_BOUND)
        {
            return false;
        }

        let dimension_type = &level.dimension_type;
        let block_light_limit = dimension_type.monster_spawn_block_light_limit;
        if block_light_limit < i32::from(MAX_LIGHT_LEVEL)
            && i32::from(level.light_value_at(LightLayer::Block, pos)) > block_light_limit
        {
            return false;
        }

        let sky_darkening = if level.is_thundering() {
            THUNDERING_SKY_DARKENING
        } else {
            level.sky_darkening()
        };
        let brightness = level.max_local_raw_brightness(pos, sky_darkening);
        i32::from(brightness)
            <= sample_monster_spawn_light_test(&dimension_type.monster_spawn_light_level, random)
    }

    /// Checks difficulty, light, and the spawn surface.
    fn check_monster_spawn_rules(
        entity_type: EntityTypeRef,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
        random: &mut impl Random,
    ) -> bool
    where
        Self: Sized,
    {
        is_spawn_allowed_on_difficulty(entity_type, level)
            && (spawn_reason.ignores_light_requirements()
                || Self::is_dark_enough_to_spawn(level, pos, random))
            && <Self as Mob>::check_mob_spawn_rules(entity_type, level, spawn_reason, pos)
    }

    /// Adds sky visibility to monster spawn rules, except for spawners.
    fn check_surface_monsters_spawn_rules(
        entity_type: EntityTypeRef,
        level: &World,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
        random: &mut impl Random,
    ) -> bool
    where
        Self: Sized,
    {
        Self::check_monster_spawn_rules(entity_type, level, spawn_reason, pos, random)
            && (spawn_reason.is_spawner() || level.can_see_sky(pos))
    }
}


/// Applies vanilla's `EntityType.canSpawn` difficulty gate here until Steel
/// has a creation/spawner gate that rejects hostile types on Peaceful.
fn is_spawn_allowed_on_difficulty(entity_type: EntityTypeRef, level: &World) -> bool {
    entity_type.allowed_in_peaceful || level.difficulty() != Difficulty::Peaceful
}

/// Samples the dimension's spawn-light provider. Vanilla uses uniform
/// providers; unknown distributions currently return their minimum.
fn sample_monster_spawn_light_test(
    level: &MonsterSpawnLightLevel,
    random: &mut impl Random,
) -> i32 {
    match level {
        MonsterSpawnLightLevel::Simple(level) => *level,
        MonsterSpawnLightLevel::Complex {
            distribution_type,
            min_inclusive,
            max_inclusive,
        } => {
            if *distribution_type == "minecraft:uniform" {
                random.next_i32_between(*min_inclusive, *max_inclusive)
            } else {
                *min_inclusive
            }
        }
    }
}
