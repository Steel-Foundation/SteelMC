//! Vanilla `Snowball`.

use crate::entity::projectile;
use crate::entity::projectile::ProjectileTable;
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::SnowballEntityData;
use steel_registry::{vanilla_entities, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;

use crate::entity::prelude::*;
use crate::entity::projectile::throwable;
use crate::entity::projectile::throwable_item::ThrownItem;
use crate::entity::projectile::{ProjectileBase, ProjectileProps};
use crate::entity::{Entity, EntityBase, EntityProps, RemovalReason};

/// A thrown snowball.
#[entity_behavior(class = "Snowball")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY)]
#[entity(key = "steel:entity/snowball", constructors)]
pub struct SnowballEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    #[entity(component)]
    thrown_item: ThrownItem,
    #[entity(synced)]
    entity_data: CheckedMutex<SnowballEntityData>,
}

impl SnowballEntity {
    const ENTITY: EntityProps<Self> = throwable::entity_props();

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = SnowballEntityData::new();
        let thrown_item = ThrownItem::new(&vanilla_items::SNOWBALL, &mut entity_data);
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(),
            thrown_item,
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// `Snowball.onHit`: breaks into particles on clients and disappears.
    fn on_hit_after_super(&self) {
        // VANILLA CLIENT-LOCAL: entity event 3 shows the break particles.
        self.broadcast_entity_event(EntityStatus::Death);
        self.set_removed(RemovalReason::Discarded);
    }
}

impl ProjectileTable for SnowballEntity {
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        on_hit: |snowball, hit| {
            projectile::on_hit(snowball, hit);
            snowball.on_hit_after_super();
        },
        on_hit_entity: |snowball, target, location| {
            projectile::on_hit_entity(snowball, target, location);
            let damage = if target.entity_type() == &vanilla_entities::BLAZE {
                3.0
            } else {
                0.0
            };
            if let Some(world) = target.level()
                && let Some(source) = snowball.projectile().thrown_damage()
            {
                target.hurt(&world, &source, damage);
            }
        },
        ..ProjectileProps::new()
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::{init_vanilla_registry, vanilla_damage_types};
    use steel_utils::{BlockPos, Direction};

    use super::*;

    use crate::entity::damage::DamageSource;
    use crate::entity::projectile::ProjectileHit;
    use crate::test_support::test_world;
    use crate::world::ClipHitResult;
    use glam::DVec3;

    fn snowball() -> Arc<SnowballEntity> {
        SnowballEntity::new(
            &vanilla_entities::SNOWBALL,
            1,
            DVec3::ZERO,
            Arc::downgrade(test_world()),
        )
    }

    #[test]
    fn starts_as_a_snowball_item() {
        init_vanilla_registry();
        assert!(
            snowball()
                .throwable_item()
                .item()
                .is(&vanilla_items::SNOWBALL)
        );
    }

    #[test]
    fn hurt_marks_the_snowball_but_never_damages_it() {
        init_vanilla_registry();
        let snowball = snowball();
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(!snowball.hurt(test_world(), &source, 1.0));
        assert!(snowball.hurt_marked());

        snowball.clear_hurt_mark();
        snowball.set_invulnerable(true);
        assert!(!snowball.hurt(test_world(), &source, 1.0));
        assert!(!snowball.hurt_marked());
    }

    #[test]
    fn hitting_a_block_discards_the_snowball() {
        init_vanilla_registry();
        let snowball = snowball();
        let hit = ProjectileHit::Block {
            location: DVec3::ZERO,
            hit: ClipHitResult {
                location: DVec3::ZERO,
                direction: Direction::Up,
                block_pos: BlockPos::new(0, 0, 0),
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        };

        snowball.projectile().on_hit(&hit);
        assert!(snowball.is_removed());
    }
}
