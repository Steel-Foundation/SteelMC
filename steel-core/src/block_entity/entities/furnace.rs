use std::sync::{Arc, Weak};
use steel_registry::{item_stack::ItemStack, vanilla_block_entity_types, vanilla_fuel_values};
use simdnbt::{borrow::BaseNbtCompound, owned::NbtCompound};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::inventory::{container::Container, lock::{ContainerRef, SharedContainer}};
use crate::world::World;

pub struct FurnaceBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<FurnaceContainer>>,
    container_ref: ContainerRef,
    data: Arc<SyncMutex<FurnaceData>>,
}

struct FurnaceContainer {
    items: Vec<ItemStack>,
}

struct FurnaceData {
    lit_time: i32,
    lit_duration: i32,
    cooking_progress: i32,
    cooking_total_time: i32,
}

unsafe impl DowncastType for FurnaceBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/furnace");
}

unsafe impl DowncastType for FurnaceContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/furnace");
}

impl FurnaceBlockEntity {
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::FURNACE,
            level,
            pos,
            state,
        ));
        // Furnace has exactly 3 slots: input (0), fuel (1), output (2)
        let container = Arc::new(SyncMutex::new(FurnaceContainer {
            items: vec![ItemStack::empty(); 3],
        }));
        let shared: SharedContainer = container.clone();
        let data = Arc::new(SyncMutex::new(FurnaceData {
            lit_time: 0,
            lit_duration: 0,
            cooking_progress: 0,
            cooking_total_time: 200,
        }));
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared, Arc::clone(&base)),
            base,
            container,
            data,
        }
    }
}

impl BlockEntity for FurnaceBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _: &BaseNbtCompound<'_>) {}

    fn save_additional(&self, _: &mut NbtCompound) {}

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn tick(&self, _world: &Arc<World>) {
        let mut container = self.container.lock();
        let mut data = self.data.lock();

        let is_lit = data.lit_time > 0;
        let mut changed = false;

        // Consume fuel time
        if is_lit {
            data.lit_time -= 1;
        }

        // Read input and fuel info without holding mutable references
        let input_empty = container.items[0].is_empty();
        let input_item = container.items[0].item();
        let fuel_empty = container.items[1].is_empty();
        let fuel_item = container.items[1].item();
        let output_count = container.items[2].count();
        let output_item = container.items[2].item();

        // Check if we can smelt
        let can_smelt = !input_empty && output_count < 64;

        if can_smelt {
            // If not lit and has fuel, light it
            if !is_lit && !fuel_empty {
                let burn_time = vanilla_fuel_values::burn_duration(fuel_item);
                if burn_time > 0 {
                    data.lit_time = burn_time;
                    data.lit_duration = burn_time;
                    container.items[1].shrink(1);
                    changed = true;
                }
            }

            // If lit, cook
            if data.lit_time > 0 {
                data.cooking_progress += 1;
                if data.cooking_progress >= data.cooking_total_time {
                    // Smelting complete - for now just duplicate the input item
                    // TODO: Use actual smelting recipes
                    if container.items[2].is_empty() {
                        container.items[2] = ItemStack::with_count(input_item, 1);
                    } else if output_item == input_item {
                        container.items[2].grow(1);
                    }
                    container.items[0].shrink(1);
                    data.cooking_progress = 0;
                    changed = true;
                }
            }
        } else {
            data.cooking_progress = 0;
        }

        if changed {
            drop(container);
            drop(data);
            self.base.set_changed();
        }
    }
}

impl Container for FurnaceContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        3
    }

    fn set_changed(&mut self) {}
}
