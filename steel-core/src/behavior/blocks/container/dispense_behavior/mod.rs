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
pub mod boat;
pub mod bucket;
pub mod consumables;
pub mod default;
pub mod potion;
pub mod projectile;
pub mod tnt;
pub mod tools;

pub use armor::ArmorDispenseBehavior;
pub use arrow::ArrowDispenseBehavior;
pub use boat::BoatDispenseBehavior;
pub use bucket::BucketDispenseBehavior;
pub use consumables::{
    BoneMealDispenseBehavior, GlowstoneDispenseBehavior, HoneycombDispenseBehavior,
};
pub use default::DefaultDispenseBehavior;
pub use potion::GlassBottleDispenseBehavior;
pub use projectile::ProjectileDispenseBehavior;
pub use tnt::TntDispenseBehavior;
pub use tools::FlintAndSteelDispenseBehavior;

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
    registry.set_behavior(&vanilla_items::BUCKET, Box::new(BucketDispenseBehavior));

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
    // TODO implement blue and brown egg entities
    registry.set_behavior(
        &vanilla_items::BLUE_EGG,
        Box::new(ProjectileDispenseBehavior::new(
            &vanilla_entities::EGG,
            1.1,
            6.0,
        )),
    );
    registry.set_behavior(
        &vanilla_items::BROWN_EGG,
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

    registry.set_behavior(
        &vanilla_items::OAK_BOAT,
        Box::new(BoatDispenseBehavior::new(&vanilla_entities::OAK_BOAT)),
    );

    registry.set_behavior(
        &vanilla_items::HONEYCOMB,
        Box::new(HoneycombDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::GLOWSTONE,
        Box::new(GlowstoneDispenseBehavior),
    );
    registry.set_behavior(
        &vanilla_items::GLASS_BOTTLE,
        Box::new(GlassBottleDispenseBehavior),
    );
    // TODO: shears, brush, carved pumpkin — see tools.rs / consumables.rs

    // TODO: shulker box + dyed shulker box (container.rs) — needs generic
    // block-item placement dispatch, which dispensers don't have yet.
    // TODO: chest (container.rs) — needs ChestBlock, which doesn't exist.
    // TODO: minecart, chest/furnace/hopper/tnt/command-block minecarts
    // (minecarts.rs) — need minecart entities plus rail-slope detection for
    // the drop-vs-place branch, neither of which is ported yet.
    // TODO: armor stand (spawn_egg.rs) — needs the ArmorStand entity.
    // TODO: plain Potion (potion.rs) — a water bottle in front of the
    // dispenser converts CONVERTABLE_TO_MUD blocks to mud; deferred pending
    // verification that the tag exists in Steel.

    registry
});
