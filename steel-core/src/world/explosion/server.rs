//! Vanilla server explosion orchestration.

use std::sync::Arc;

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_registry::{vanilla_damage_types, vanilla_entities, vanilla_game_events};

use crate::entity::damage::DamageSource;
use crate::entity::{Entity, SharedEntity};
use crate::world::World;
use crate::world::game_event::GameEventContext;

use super::{
    BlockInteraction, Explosion, ExplosionDamageCalculator, ImmutableExplosionBlockCalculator,
    SelectedDamageCalculator,
};

mod block_effects;
mod block_rays;
mod entity_effects;
mod exposure;

const SMALL_EXPLOSION_RADIUS: f32 = 2.0;

#[cfg(test)]
use block_rays::RAY_COUNT;
#[cfg(test)]
use exposure::{EntityExplosionExposure, seen_percent};

pub(super) struct ServerExplosion<'a> {
    world: &'a Arc<World>,
    fire: bool,
    block_interaction: BlockInteraction,
    center: DVec3,
    source: Option<SharedEntity>,
    indirect_source: Option<SharedEntity>,
    radius: f32,
    damage_source: DamageSource,
    damage_calculator: SelectedDamageCalculator<'a>,
    immutable_block_calculator: Option<&'a dyn ImmutableExplosionBlockCalculator>,
    pub(super) hit_players: FxHashMap<i32, DVec3>,
}

impl<'a> ServerExplosion<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the Vanilla ServerExplosion construction boundary"
    )]
    pub(super) fn new(
        world: &'a Arc<World>,
        source: Option<SharedEntity>,
        damage_source: Option<DamageSource>,
        damage_calculator: Option<&'a dyn ExplosionDamageCalculator>,
        immutable_block_calculator: Option<&'a dyn ImmutableExplosionBlockCalculator>,
        center: DVec3,
        radius: f32,
        fire: bool,
        block_interaction: BlockInteraction,
    ) -> Self {
        let indirect_source = source.as_ref().and_then(|source| {
            if source.as_living_entity().is_some() {
                Some(Arc::clone(source))
            } else {
                source.explosion_indirect_source()
            }
        });
        let damage_source = damage_source
            .unwrap_or_else(|| default_damage_source(source.clone(), indirect_source.clone()));
        let damage_calculator = match (damage_calculator, &source) {
            (Some(calculator), _) => SelectedDamageCalculator::Custom(calculator),
            (None, Some(source)) => SelectedDamageCalculator::Entity(Arc::clone(source)),
            (None, None) => SelectedDamageCalculator::Default,
        };
        Self {
            world,
            fire,
            block_interaction,
            center,
            source,
            indirect_source,
            radius,
            damage_source,
            damage_calculator,
            immutable_block_calculator,
            hit_players: FxHashMap::default(),
        }
    }

    pub(super) fn explode(&mut self) -> usize {
        self.world.game_event_at(
            &vanilla_game_events::EXPLODE,
            self.center,
            &GameEventContext::new(self.source.as_deref(), None),
        );
        let mut affected = self.calculate_exploded_positions_from_level_random();
        self.hurt_entities();
        if self.interacts_with_blocks() {
            self.interact_with_blocks(&mut affected);
        }
        if self.fire {
            self.create_fire(&affected);
        }
        affected.len()
    }

    fn interacts_with_blocks(&self) -> bool {
        self.block_interaction != BlockInteraction::Keep
    }

    pub(super) fn is_small(&self) -> bool {
        self.radius < SMALL_EXPLOSION_RADIUS || !self.interacts_with_blocks()
    }
}

impl Explosion for ServerExplosion<'_> {
    fn world(&self) -> &Arc<World> {
        self.world
    }

    fn damage_source(&self) -> &DamageSource {
        &self.damage_source
    }

    fn block_interaction(&self) -> BlockInteraction {
        self.block_interaction
    }

    fn indirect_source_entity(&self) -> Option<&dyn Entity> {
        self.indirect_source.as_deref()
    }

    fn direct_source_entity(&self) -> Option<&dyn Entity> {
        self.source.as_deref()
    }

    fn radius(&self) -> f32 {
        self.radius
    }

    fn center(&self) -> DVec3 {
        self.center
    }

    fn should_affect_blocklike_entities(&self) -> bool {
        let is_wind_charge = self.source.as_ref().is_some_and(|source| {
            source.entity_type() == &vanilla_entities::BREEZE_WIND_CHARGE
                || source.entity_type() == &vanilla_entities::WIND_CHARGE
        });
        !is_wind_charge
            && (self.world.get_game_rule(&MOB_GRIEFING)
                || self.block_interaction.should_affect_blocklike_entities())
    }
}

/// Vanilla `DamageSources.explosion(direct, indirect)`.
fn default_damage_source(
    direct: Option<SharedEntity>,
    indirect: Option<SharedEntity>,
) -> DamageSource {
    let damage_type = if direct.is_some() && indirect.is_some() {
        &vanilla_damage_types::PLAYER_EXPLOSION
    } else {
        &vanilla_damage_types::EXPLOSION
    };
    let mut damage_source = DamageSource::environment(damage_type);
    if let Some(direct) = direct {
        damage_source = damage_source.with_direct_entity(direct);
    }
    if let Some(indirect) = indirect {
        damage_source = damage_source.with_causing_entity(indirect);
    }
    damage_source
}

#[cfg(test)]
mod tests;
