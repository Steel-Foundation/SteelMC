use std::sync::{
    Arc, Weak,
    atomic::{AtomicI32, Ordering},
};

use glam::DVec3;
use simdnbt::{
    ToNbtTag,
    borrow::{BaseNbtCompound, NbtCompound as NbtCompoundView},
    owned::{NbtCompound, NbtList, NbtTag},
};
use steel_registry::{
    ItemStackTemplate, REGISTRY,
    blocks::{
        behavior::PushReaction, block_state_ext::BlockStateExt, properties::BlockStateProperties,
    },
    data_components::{DataComponentPatch, ItemContainerContents, vanilla_components::CONTAINER},
    item_stack::ItemStack,
    vanilla_block_entity_types,
};
use steel_utils::{
    BlockLocalAabb, BlockPos, BlockStateId, Direction, DowncastType, DowncastTypeKey, WorldAabb,
    geometry::{Aabb, Space},
    locks::SyncMutex,
    types::UpdateFlags,
};

use crate::{
    behavior::blocks::ShulkerBoxBlock,
    block_entity::{BlockEntity, BlockEntityBase},
    inventory::{
        container::Container,
        lock::{ContainerRef, SharedContainer},
    },
    physics::{CollisionWorld as _, MoverType, WorldCollisionProvider},
    world::World,
};

/// Number of slots in a shulker box (3 rows of 9).
pub const SHULKER_BOX_SLOTS: usize = 27;
const ANIMATION_STEPS: u8 = 10;

/// The current animation state of a shulker box.
#[derive(Debug, Clone, Copy)]
pub enum AnimationStatus {
    /// Fully closed.
    Closed,
    /// Opening.
    Opening,
    /// Fully open.
    Opened,
    /// Closing.
    Closing,
}

pub struct ShulkerBoxAnimation {
    animation_status: AnimationStatus,
    progress: u8,
    old_progress: u8,
}

impl ShulkerBoxAnimation {
    fn progress(&self) -> f32 {
        f32::from(self.progress) / f32::from(ANIMATION_STEPS)
    }

    fn old_progress(&self) -> f32 {
        f32::from(self.old_progress) / f32::from(ANIMATION_STEPS)
    }
}

/// Block entity backing every shulker box color, including the undyed one.
pub struct ShulkerBoxBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<ShulkerBoxContainer>>,
    container_ref: ContainerRef,
    animation: SyncMutex<ShulkerBoxAnimation>,
    open_count: AtomicI32,
}

struct ShulkerBoxContainer {
    items: Vec<ItemStack>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ShulkerBoxEntity`.
unsafe impl DowncastType for ShulkerBoxBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/shulker_box");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a shulker box block entity.
unsafe impl DowncastType for ShulkerBoxContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/shulker_box");
}

fn do_neighbor_updates(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    world.update_neighbour_shapes(state, pos, UpdateFlags::UPDATE_ALL, 512);
    world.update_neighbors_at(pos, state.get_block());
}

impl ShulkerBoxBlockEntity {
    /// Creates a new shulker box block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::SHULKER_BOX,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(ShulkerBoxContainer {
            items: vec![ItemStack::empty(); SHULKER_BOX_SLOTS],
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
            animation: SyncMutex::new(ShulkerBoxAnimation {
                animation_status: AnimationStatus::Closed,
                progress: 0,
                old_progress: 0,
            }),
            open_count: AtomicI32::new(0),
        }
    }

    fn update_animation(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let mut animation = self.animation.lock();
        animation.old_progress = animation.progress;
        match animation.animation_status {
            AnimationStatus::Closed => animation.progress = 0,
            AnimationStatus::Opening => {
                animation.progress += 1;
                if animation.old_progress == 0 {
                    do_neighbor_updates(world, pos, state);
                }

                if animation.progress >= ANIMATION_STEPS {
                    animation.animation_status = AnimationStatus::Opened;
                    animation.progress = ANIMATION_STEPS;
                    do_neighbor_updates(world, pos, state);
                }

                drop(animation);
                self.move_collided_entities(world, pos, state);
            }
            AnimationStatus::Opened => animation.progress = ANIMATION_STEPS,
            AnimationStatus::Closing => {
                animation.progress = animation.progress.saturating_sub(1);
                if animation.old_progress == ANIMATION_STEPS {
                    do_neighbor_updates(world, pos, state);
                }

                if animation.progress == 0 {
                    animation.animation_status = AnimationStatus::Closed;
                    animation.progress = 0;
                    do_neighbor_updates(world, pos, state);
                }
            }
        }
    }

    fn move_collided_entities(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let direction = state.get_value(&BlockStateProperties::FACING);

        let aabb: WorldAabb = {
            let animation = self.animation.lock();
            Self::get_progress_delta_aabb(
                1.0,
                direction,
                animation.old_progress(),
                animation.progress(),
                DVec3::from(pos.get_bottom_center()),
            )
        };

        let entities = world.get_entities_in_aabb(&aabb);
        for entity in entities {
            if entity.piston_push_reaction() == PushReaction::Ignore {
                continue;
            }
            let (offset_x, offset_y, offset_z) = direction.offset();
            entity.move_entity(
                MoverType::ShulkerBox,
                DVec3::new(
                    (aabb.width() + 0.01) * f64::from(offset_x),
                    (aabb.height() + 0.01) * f64::from(offset_y),
                    (aabb.depth() + 0.01) * f64::from(offset_z),
                ),
            );
        }
    }

    /// Builds the item form of a placed, possibly-filled shulker box.
    /// Vanilla `ShulkerBoxBlockEntity.collectComponents()` +
    /// `BaseContainerBlockEntity.collectImplicitComponents(CONTAINER)`.
    pub fn shulker_box_as_item(&self, state: BlockStateId) -> ItemStack {
        let block_item = REGISTRY.items.by_block(state.get_block());

        let contents = self.collect_components();

        let mut patch = DataComponentPatch::new();
        patch.set(CONTAINER, contents);

        ItemStack::with_count_and_patch(block_item, 1, patch)
    }

    /// Calculates the bounding box of a shulker during its opening animation.
    ///
    /// The bounds are expanded in the opening direction based on the animation progress.
    #[must_use]
    pub fn get_progress_delta_aabb<I: Space>(
        size: f32,
        direction: Direction,
        progress_from: f32,
        progress_to: f32,
        position: DVec3,
    ) -> Aabb<DVec3, I> {
        let size = f64::from(size);
        let bounds =
            Aabb::<DVec3, I>::new(-size * 0.5, 0.0, -size * 0.5, size * 0.5, size, size * 0.5);

        let max_movement = f64::from(progress_from.max(progress_to));
        let min_movement = f64::from(progress_from.min(progress_to));

        let dir = DVec3::from(direction.offset_vec());

        bounds
            .expand_towards(dir * max_movement * size)
            .contract(-dir * (1.0 + min_movement) * size)
            .translate(position)
    }

    /// Block-local bounds of the box including the lid at its current progress.
    #[must_use]
    pub fn get_bounding_box(&self, state: BlockStateId) -> BlockLocalAabb {
        let bottom_center = DVec3::new(0.5, 0.0, 0.5);
        Self::get_progress_delta_aabb(
            1.0,
            state.get_value(ShulkerBoxBlock::FACING),
            -1.0,
            0.5 * self.progress(1.0),
            bottom_center,
        )
    }

    /// Returns whether the shulker box can open
    #[must_use]
    pub fn can_open(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        if !matches!(self.animation_status(), AnimationStatus::Closed) {
            return true;
        }

        let direction = state.get_value(ShulkerBoxBlock::FACING);

        let lid_open_bounding_box: WorldAabb = Self::get_progress_delta_aabb(
            1.0,
            direction,
            0.0,
            0.5,
            DVec3::from(pos.get_bottom_center()),
        )
        .deflate(1.0E-6);

        let collision = WorldCollisionProvider::new(world);
        !collision.has_block_collision(&lid_open_bounding_box)
    }

    /// Checks if the block entity's container has any items
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.container.lock().is_empty()
    }

    /// Collect all the items inside the shulker box into an `ItemContainerContents`
    ///
    /// # Panics
    /// Panics if shulker box somehow has more than 256 slots. This should never happen
    #[must_use]
    pub fn collect_components(&self) -> ItemContainerContents {
        let container = self.container.lock();

        let size = container.get_container_size();
        let slots: Vec<Option<ItemStackTemplate>> = (0..size)
            .map(|i| {
                let item = container.get_item(i);
                if item.is_empty() {
                    None
                } else {
                    ItemStackTemplate::from_stack(item).ok()
                }
            })
            .collect();

        ItemContainerContents::new(slots)
            .expect("shulker box slot count is always within the 256 container-contents limit")
    }

    /// Interpolated progress for smooth rendering between ticks.
    #[must_use]
    pub fn progress(&self, partial_tick: f32) -> f32 {
        let animation = self.animation.lock();
        let new = animation.progress();
        let old = animation.old_progress();
        old + partial_tick * (new - old)
    }

    /// Get the current animation state
    #[must_use]
    pub fn animation_status(&self) -> AnimationStatus {
        self.animation.lock().animation_status
    }
}

impl BlockEntity for ShulkerBoxBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn tick(&self, world: &Arc<World>) {
        self.update_animation(world, self.base.pos, self.base.block_state());
    }

    fn trigger_event(&self, kind: i32, data: i32) -> bool {
        if kind == 1 {
            self.open_count.store(data, Ordering::Relaxed);
            if data == 0 {
                self.animation.lock().animation_status = AnimationStatus::Closing;
            }

            if data == 1 {
                self.animation.lock().animation_status = AnimationStatus::Opening;
            }

            true
        } else {
            false
        }
    }

    fn load_additional(&self, nbt: &BaseNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());

        let Some(items_list) = nbt_view.list("Items") else {
            return;
        };
        let Some(compounds) = items_list.compounds() else {
            return;
        };

        for compound in compounds {
            let Some(slot) = compound.byte("Slot") else {
                continue;
            };
            let slot = slot as usize;
            if slot >= SHULKER_BOX_SLOTS {
                continue;
            }
            if let Some(item) = ItemStack::from_borrowed_compound(&compound) {
                container.items[slot] = item;
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items().iter().enumerate() {
            if item.is_empty() {
                continue;
            }
            let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag() else {
                continue;
            };
            item_nbt.insert("Slot", slot as i8);
            items.push(item_nbt);
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    fn apply_components_from_item(&self, item: &ItemStack) {
        let Some(contents) = item.get(CONTAINER) else {
            return;
        };

        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        for (slot, template) in contents.items().iter().enumerate() {
            if slot >= SHULKER_BOX_SLOTS {
                break;
            }
            if let Some(template) = template {
                container.items_mut()[slot] = ItemStack::with_count_and_patch(
                    template.item(),
                    template.count(),
                    template.components().clone(),
                );
            }
        }
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }
}

impl Container for ShulkerBoxContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        SHULKER_BOX_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < SHULKER_BOX_SLOTS {
            let max_stack_size = self.get_max_stack_size_for_item(&stack);
            if !stack.is_empty() && stack.count() > max_stack_size {
                stack.set_count(max_stack_size);
            }
            self.items[slot] = stack;
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {}
}
