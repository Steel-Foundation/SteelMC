//! Player-thrown wind charge.

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::ExplosionParticlePalette;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_event::SoundEventHolder;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::WindChargeEntityData;
use steel_registry::{sound_events, vanilla_particle_types};
use steel_utils::locks::SyncMutex;

use crate::entity::prelude::*;
use crate::entity::projectile::{self, ProjectileTable, hurting, wind_charge};
use crate::entity::{
    AbstractWindCharge, EntityBase, EntityProps, EntityReference, EntityTable,
    HurtingProjectileBase, ProjectileBase, ProjectileDeflection, ProjectileProps,
};
use crate::world::{ExplosionInteraction, ExplosionOptions, SimpleExplosionDamageCalculator};

const EXPLOSION_RADIUS: f32 = 1.2;
const EXPLOSION_KNOCKBACK_MULTIPLIER: f32 = 1.22;
const NO_DEFLECT_TICKS: i32 = 5;
const INERTIA: f32 = 1.0;

static EXPLOSION_DAMAGE_CALCULATOR: SimpleExplosionDamageCalculator =
    SimpleExplosionDamageCalculator::new(
        true,
        false,
        Some(EXPLOSION_KNOCKBACK_MULTIPLIER),
        Some(BlockTag::BLOCKS_WIND_CHARGE_EXPLOSIONS),
    );

/// Wind charge thrown by a player or shot from a dispenser.
#[entity_behavior(class = "WindCharge")]
#[derive(Entity)]
#[entity(key = "steel:entity/wind_charge", constructors)]
pub struct WindChargeEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    hurting_projectile: HurtingProjectileBase,
    #[entity(synced)]
    entity_data: SyncMutex<WindChargeEntityData>,
    no_deflect_ticks: SyncMutex<i32>,
}

impl WindChargeEntity {
    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let entity = Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(),
            hurting_projectile: HurtingProjectileBase::new(0.0),
            entity_data: SyncMutex::new(WindChargeEntityData::new()),
            no_deflect_ticks: SyncMutex::new(NO_DEFLECT_TICKS),
        };
        entity
            .base
            .set_bounding_box(wind_charge::make_bounding_box_at(
                &entity,
                entity.position(),
            ));
        entity
    }

    fn entity_tick(&self) {
        wind_charge::tick(self);
        let mut ticks = self.no_deflect_ticks.lock();
        if *ticks > 0 {
            *ticks -= 1;
        }
    }

    fn projectile_deflect_with_power(
        &self,
        deflection: ProjectileDeflection,
        deflecting_entity: Option<&dyn Entity>,
        new_owner: Option<EntityReference>,
        by_attack: bool,
        power: DVec3,
    ) -> bool {
        if *self.no_deflect_ticks.lock() > 0 {
            return false;
        }
        projectile::deflect_with_power(
            self,
            deflection,
            deflecting_entity,
            new_owner,
            by_attack,
            power,
        )
    }
}

impl EntityTable for WindChargeEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        ..wind_charge::entity_props()
    };
}

impl ProjectileTable for WindChargeEntity {
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        deflect_with_power: Self::projectile_deflect_with_power,
        ..wind_charge::projectile_props()
    };
}

impl hurting::Parts for WindChargeEntity {
    const SHOULD_BURN: bool = false;
    const INERTIA: f32 = INERTIA;
    const LIQUID_INERTIA: f32 = INERTIA;
}

impl AbstractWindCharge for WindChargeEntity {
    fn explode(&self, position: DVec3) {
        let Some(world) = self.level() else {
            return;
        };
        let mut options =
            ExplosionOptions::new(position, EXPLOSION_RADIUS, ExplosionInteraction::Trigger);
        options.source = self.shared();
        options.damage_calculator = Some(&EXPLOSION_DAMAGE_CALCULATOR);
        options.immutable_block_calculator = Some(&EXPLOSION_DAMAGE_CALCULATOR);
        options.small_explosion_particle =
            ParticleData::simple(&vanilla_particle_types::GUST_EMITTER_SMALL);
        options.large_explosion_particle =
            ParticleData::simple(&vanilla_particle_types::GUST_EMITTER_LARGE);
        options.block_particles = ExplosionParticlePalette::empty();
        options.explosion_sound =
            SoundEventHolder::registry(&sound_events::ENTITY_WIND_CHARGE_WIND_BURST);
        world.explode(options);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::component::Components as _;
    use crate::entity::entities::PigEntity;
    use crate::entity::{INITIAL_ACCELERATION_POWER, SharedEntity, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::{LevelReader as _, World};

    const OPEN: &BoolProperty = &BlockStateProperties::OPEN;

    #[test]
    fn deflection_is_refused_while_fresh_and_attacks_restore_acceleration() {
        init_vanilla_registry();

        let charge = WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        charge.set_velocity(DVec3::X);

        assert!(
            !charge
                .projectile()
                .deflect(ProjectileDeflection::Reverse, None, None, false)
        );
        assert_eq!(charge.velocity(), DVec3::X);

        *charge.no_deflect_ticks.lock() = 0;
        assert!(
            charge
                .projectile()
                .deflect(ProjectileDeflection::None, None, None, true)
        );
        assert_eq!(
            charge.get::<HurtingProjectileBase>().acceleration_power(),
            INITIAL_ACCELERATION_POWER
        );
        assert!(
            charge
                .projectile()
                .deflect(ProjectileDeflection::None, None, None, false)
        );
        assert_eq!(
            charge.get::<HurtingProjectileBase>().acceleration_power(),
            INITIAL_ACCELERATION_POWER * 0.5
        );
    }

    #[test]
    fn block_hit_bursts_and_knocks_back_nearby_entities_without_damage() {
        init_vanilla_registry();
        init_behaviors();

        let fixture = fresh_test_world("wind_charge_block_hit");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(8, 63, 8),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let pig = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(9.25, 64.0, 8.5),
            Arc::downgrade(world),
        );
        world
            .try_add_entity(Arc::clone(&pig) as SharedEntity)
            .expect("pig should attach to the loaded chunk");
        let health = pig.living().health();

        let charge = WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            next_entity_id(),
            DVec3::new(8.5, 65.0, 8.5),
            Arc::downgrade(world),
        );
        charge.set_velocity(DVec3::new(0.0, -1.5, 0.0));
        world
            .try_add_entity(Arc::clone(&charge) as SharedEntity)
            .expect("wind charge should attach to the loaded chunk");

        charge.tick();

        assert!(charge.is_removed());
        assert!(
            pig.velocity().x > 0.0,
            "the burst should push the pig away from the impact point"
        );
        assert_eq!(pig.living().health(), health);
    }

    #[test]
    fn burst_toggles_wind_openable_trapdoors_only() {
        init_vanilla_registry();
        init_behaviors();

        let fixture = fresh_test_world("wind_charge_trapdoors");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(8, 63, 8),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let oak_pos = BlockPos::new(9, 64, 8);
        let iron_pos = BlockPos::new(7, 64, 8);
        for (pos, block) in [
            (oak_pos, &vanilla_blocks::OAK_TRAPDOOR),
            (iron_pos, &vanilla_blocks::IRON_TRAPDOOR),
        ] {
            assert!(world.set_block(pos, block.default_state(), UpdateFlags::UPDATE_NONE));
        }

        let charge = WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            next_entity_id(),
            DVec3::new(8.5, 65.0, 8.5),
            Arc::downgrade(world),
        );
        charge.set_velocity(DVec3::new(0.0, -1.5, 0.0));
        world
            .try_add_entity(Arc::clone(&charge) as SharedEntity)
            .expect("wind charge should attach to the loaded chunk");

        charge.tick();

        assert!(charge.is_removed());
        assert!(world.get_block_state(oak_pos).get_value(OPEN));
        assert!(!world.get_block_state(iron_pos).get_value(OPEN));
    }

    #[test]
    fn non_player_owner_restricts_block_breaking_to_can_break_predicate() {
        init_vanilla_registry();

        let fixture = fresh_test_world("wind_charge_adventure_break");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let pos = BlockPos::new(8, 64, 8);

        let charge = WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            next_entity_id(),
            DVec3::new(8.5, 66.0, 8.5),
            Arc::downgrade(world),
        );
        world
            .try_add_entity(Arc::clone(&charge) as SharedEntity)
            .expect("wind charge should attach to the loaded chunk");
        assert!(charge.projectile().may_break(world, pos));

        let pig: SharedEntity = PigEntity::new(
            &vanilla_entities::PIG,
            next_entity_id(),
            DVec3::new(9.5, 64.0, 8.5),
            Arc::downgrade(world),
        );
        world
            .try_add_entity(Arc::clone(&pig))
            .expect("pig should attach to the loaded chunk");
        charge.projectile().set_owner(Some(&pig));

        assert!(!charge.projectile().may_break(world, pos));
    }

    #[test]
    fn local_position_keeps_lowered_bounding_box() {
        init_vanilla_registry();

        let charge = WindChargeEntity::new(
            &vanilla_entities::WIND_CHARGE,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        let position = DVec3::new(4.0, 70.0, -3.0);
        charge.set_position_local(position);

        assert_eq!(
            charge.bounding_box(),
            wind_charge::make_bounding_box_at(charge.as_ref(), position)
        );
    }
}
