use std::sync::{Arc, LazyLock};
use steel_registry::data_components::vanilla_components::EQUIPPABLE;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_items;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use crate::world::World;

pub mod armor;
pub mod arrow;
pub mod bone_meal;
pub mod bucket;
pub mod default;
pub mod flint_and_steel;
pub mod projectile;
pub mod tnt;

pub use armor::ArmorDispenseBehavior;
pub use arrow::ArrowDispenseBehavior;
pub use bone_meal::BoneMealDispenseBehavior;
pub use bucket::BucketDispenseBehavior;
pub use default::DefaultDispenseBehavior;
pub use flint_and_steel::FlintAndSteelDispenseBehavior;
pub use projectile::ProjectileDispenseBehavior;
pub use tnt::TntDispenseBehavior;

pub trait DispenseItemBehavior: Send + Sync {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: ItemStack,
    ) -> ItemStack;
}

pub struct DispenseBehaviorRegistry {
    behaviors: Vec<Box<dyn DispenseItemBehavior>>,
}

impl DispenseBehaviorRegistry {
    pub fn new() -> Self {
        let item_count = REGISTRY.items.len();
        let behaviors = (0..item_count)
            .map(|_| Box::new(DefaultDispenseBehavior) as Box<dyn DispenseItemBehavior>)
            .collect();

        Self { behaviors }
    }

    pub fn set_behavior(&mut self, item: ItemRef, behavior: Box<dyn DispenseItemBehavior>) {
        let id = item.id();
        self.behaviors[id] = behavior;
    }

    pub fn get_behavior(&self, item: ItemRef) -> &dyn DispenseItemBehavior {
        let id = item.id();
        self.behaviors[id].as_ref()
    }
}

pub static DISPENSE_BEHAVIORS: LazyLock<DispenseBehaviorRegistry> = LazyLock::new(|| {
    let mut registry = DispenseBehaviorRegistry::new();

    for (_, item) in REGISTRY.items.iter() {
        if item.components.has(EQUIPPABLE) {
            registry.set_behavior(item, Box::new(ArmorDispenseBehavior));
        }
    }
    registry.set_behavior(
        &vanilla_items::WATER_BUCKET,
        Box::new(BucketDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::LAVA_BUCKET,
        Box::new(BucketDispenseBehavior),
    );

    registry.set_behavior(
        &vanilla_items::BONE_MEAL,
        Box::new(BoneMealDispenseBehavior),
    );

    registry.set_behavior(
        &vanilla_items::FLINT_AND_STEEL,
        Box::new(FlintAndSteelDispenseBehavior),
    );

    registry.set_behavior(&vanilla_items::TNT, Box::new(TntDispenseBehavior));

    registry.set_behavior(
        &vanilla_items::ARROW,
        Box::new(ArrowDispenseBehavior::new(&vanilla_entities::ARROW)),
    );
    registry.set_behavior(
        &vanilla_items::SPECTRAL_ARROW,
        Box::new(ArrowDispenseBehavior::new(
            &vanilla_entities::SPECTRAL_ARROW,
        )),
    );
    registry.set_behavior(
        &vanilla_items::TIPPED_ARROW,
        Box::new(ArrowDispenseBehavior::new(&vanilla_entities::ARROW)),
    );

    // Projectiles
    registry.set_behavior(
        &vanilla_items::EGG,
        Box::new(ProjectileDispenseBehavior::new(
            &vanilla_entities::EGG,
            1.1,
            6.0,
        )),
    );
    registry.set_behavior(
        &vanilla_items::SNOWBALL,
        Box::new(ProjectileDispenseBehavior::new(
            &vanilla_entities::SNOWBALL,
            1.1,
            6.0,
        )),
    );
    registry.set_behavior(
        &vanilla_items::ENDER_PEARL,
        Box::new(ProjectileDispenseBehavior::new(
            &vanilla_entities::ENDER_PEARL,
            1.1,
            6.0,
        )),
    );

    registry
});
