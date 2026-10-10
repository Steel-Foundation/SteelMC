//! Shared vanilla `VehicleEntity` damage handling.

use crate::entity::{DamageSource, Entity, RemovalReason};
use crate::world::World;
use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_game_events;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;

const VEHICLE_DESTROY_DAMAGE_THRESHOLD: f32 = 40.0;
const VEHICLE_DIMENSION_CHANGING_DELAY: i32 = 10;

/// Implemented by vehicle entities (minecarts, boats) for vanilla
/// `VehicleEntity` damage handling.
pub trait VehicleEntity: Entity {
    /// Returns the vehicle's remaining hurt time.
    fn hurt_time(&self) -> i32;

    /// Sets the vehicle's remaining hurt time.
    fn set_hurt_time(&self, value: i32);

    /// Returns the vehicle's hurt direction animation.
    fn hurt_dir(&self) -> i32;

    /// Sets the vehicle's hurt direction animation.
    fn set_hurt_dir(&self, value: i32);

    /// Returns the accumulated vehicle damage.
    fn damage(&self) -> f32;

    /// Sets the accumulated vehicle damage.
    fn set_damage(&self, value: f32);

    /// Returns the item dropped when this vehicle is destroyed.
    fn vehicle_drop_item(&self) -> ItemRef;

    /// Returns whether the `source` should destroy this vehicle.
    fn should_source_destroy(&self, _source: &DamageSource) -> bool {
        false
    }

    #[expect(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "negated comparisons preserve Vanilla's NaN branch behavior"
    )]
    /// Applies vehicle damage, updates the hurt animation, and handles destruction.
    fn vehicle_hurt(&self, world: &World, source: &DamageSource, damage: f32) -> bool {
        if self.is_removed() {
            return true;
        }

        if self.is_invulnerable_to_base(source) {
            return false;
        }
        self.set_hurt_dir(-self.hurt_dir());
        self.set_hurt_time(10);
        self.mark_hurt();
        let new_damage = self.damage() + damage * 10.0;
        self.set_damage(new_damage);

        let source_entity = source.causing_entity();
        self.game_event_with_source_entity(
            &vanilla_game_events::ENTITY_DAMAGE,
            source_entity.map(AsRef::as_ref),
        );

        let creative_player = source_entity
            .and_then(|entity| entity.as_player())
            .is_some_and(|player| player.abilities.lock().instabuild);

        if (creative_player || !(new_damage > VEHICLE_DESTROY_DAMAGE_THRESHOLD))
            && !self.should_source_destroy(source)
        {
            if creative_player {
                self.set_removed(RemovalReason::Discarded);
            }
        } else {
            self.vehicle_destroy(world, source);
        }
        true
    }

    /// Destroys the vehicle using its default drop item.
    fn vehicle_destroy(&self, world: &World, _source: &DamageSource) {
        self.vehicle_destroy_with_item(world, self.vehicle_drop_item());
    }

    /// Destroys the vehicle and drops the given item, when entity drops are enabled.
    fn vehicle_destroy_with_item(&self, world: &World, drop_item: ItemRef) {
        self.kill(world);
        if world.get_game_rule(&ENTITY_DROPS) {
            let mut item = ItemStack::new(drop_item);
            if let Some(name) = self.custom_name() {
                item.set(CUSTOM_NAME, name);
            }
            self.spawn_at_location(item, 0.0);
        }
    }

    /// Returns the default dimension changing delay for vehicles.
    fn vehicle_dimension_changing_delay(&self) -> i32 {
        VEHICLE_DIMENSION_CHANGING_DELAY
    }

    // TODO: Implement ignoreExplosion.
}
