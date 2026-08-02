use std::sync::Arc;

use steel_registry::item_stack::ItemStack;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::{
    inventory::{
        lock::{ContainerLockGuard, ContainerRef},
        slots::{NormalSlot, Slot, SlotStorage},
    },
    player::Player,
};

type MayPlace = Box<dyn Fn(usize, &ItemStack) -> bool + Send + Sync>;
type MayPickup = Box<dyn Fn(usize, &ItemStack, &ContainerLockGuard, &Player) -> bool + Send + Sync>;

/// The predicates behind a [`RestrictedSlot`], shared by every slot of a
/// section so a slot costs one pointer rather than one per predicate.
pub struct RestrictedRules {
    may_place: MayPlace,
    may_pickup: Option<MayPickup>,
}

impl RestrictedRules {
    pub(crate) fn place_only(
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            may_place: Box::new(may_place),
            may_pickup: None,
        })
    }

    pub(crate) fn guarded(
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
        may_pickup: impl Fn(usize, &ItemStack, &ContainerLockGuard, &Player) -> bool
        + Send
        + Sync
        + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            may_place: Box::new(may_place),
            may_pickup: Some(Box::new(may_pickup)),
        })
    }
}

/// A [`NormalSlot`] with custom place and pickup rules.
pub struct RestrictedSlot {
    base: NormalSlot,
    rules: Arc<RestrictedRules>,
}

// SAFETY: This key uniquely identifies Steel's `RestrictedSlot`.
unsafe impl DowncastType for RestrictedSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/restricted");
}

impl RestrictedSlot {
    /// Placement gated by `may_place`, which receives the container-local slot
    /// index; pickup stays allowed.
    pub fn new(
        container: impl Into<ContainerRef>,
        index: usize,
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::with_rules(container, index, RestrictedRules::place_only(may_place))
    }

    /// Like [`new`](Self::new), but pickup is gated too.
    pub fn guarded(
        container: impl Into<ContainerRef>,
        index: usize,
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
        may_pickup: impl Fn(usize, &ItemStack, &ContainerLockGuard, &Player) -> bool
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::with_rules(
            container,
            index,
            RestrictedRules::guarded(may_place, may_pickup),
        )
    }

    /// Shares one rules object across a whole section's slots.
    pub(crate) fn with_rules(
        container: impl Into<ContainerRef>,
        index: usize,
        rules: Arc<RestrictedRules>,
    ) -> Self {
        Self {
            base: NormalSlot::new(container, index),
            rules,
        }
    }
}

impl Slot for RestrictedSlot {
    fn storage(&self) -> &SlotStorage {
        self.base.storage()
    }

    fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack {
        self.base.get_item(guard)
    }

    fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack {
        self.base.get_item_mut(guard)
    }

    fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack) {
        self.base.set_item(guard, stack);
    }

    fn may_place(&self, stack: &ItemStack) -> bool {
        (self.rules.may_place)(self.base.get_container_slot(), stack)
    }

    fn may_pickup(&self, guard: &ContainerLockGuard, player: &Player) -> bool {
        self.rules.may_pickup.as_ref().is_none_or(|it| {
            it(
                self.base.get_container_slot(),
                self.base.get_item(guard),
                guard,
                player,
            )
        })
    }

    fn get_max_stack_size(&self, guard: &ContainerLockGuard) -> i32 {
        self.base.get_max_stack_size(guard)
    }

    fn set_changed(&self, guard: &mut ContainerLockGuard) {
        self.base.set_changed(guard);
    }

    fn get_container_slot(&self) -> usize {
        self.base.get_container_slot()
    }
}

#[cfg(test)]
mod tests {
    use std::slice;
    use std::sync::Arc;

    use super::RestrictedSlot;
    use crate::inventory::container::{Container, SimpleContainer};
    use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
    use crate::inventory::slots::Slot as _;
    use steel_registry::data_components::vanilla_components::MAX_STACK_SIZE;
    use steel_registry::{item_stack::ItemStack, test_support::init_test_registry, vanilla_items};
    use steel_utils::locks::{IntoShared as _, SyncMutex};
    use steel_utils::{DowncastType, DowncastTypeKey};

    struct SingleItemContainer {
        item: ItemStack,
        max_stack_size: i32,
    }

    // SAFETY: This test-only key uniquely identifies `SingleItemContainer`.
    unsafe impl DowncastType for SingleItemContainer {
        const TYPE_KEY: DowncastTypeKey =
            DowncastTypeKey::new("steel:test/container/restricted_slot_single_item");
    }

    impl Container for SingleItemContainer {
        fn items(&self) -> &[ItemStack] {
            slice::from_ref(&self.item)
        }

        fn items_mut(&mut self) -> &mut [ItemStack] {
            slice::from_mut(&mut self.item)
        }

        fn get_max_stack_size(&self) -> i32 {
            self.max_stack_size
        }

        fn set_changed(&mut self) {}
    }

    #[test]
    fn max_stack_size_delegates_to_the_container_and_item() {
        init_test_registry();
        let capped = Arc::new(SyncMutex::new(SingleItemContainer {
            item: ItemStack::empty(),
            max_stack_size: 1,
        }));
        let capped_ref = ContainerRef::from(capped);
        let capped_slot = RestrictedSlot::new(capped_ref.clone(), 0, |_, _| true);
        let mut capped_guard = ContainerLockGuard::lock_all(&[capped_ref]);
        let capped_remainder = capped_slot.safe_insert(
            &mut capped_guard,
            ItemStack::with_count(&vanilla_items::STONE, 64),
            64,
        );
        assert_eq!(capped_slot.get_item(&capped_guard).count(), 1);
        assert_eq!(capped_remainder.count(), 63);

        let default = SimpleContainer::new(1).into_shared();
        let default_ref = ContainerRef::from(default);
        let default_slot = RestrictedSlot::new(default_ref.clone(), 0, |_, _| true);
        let mut default_guard = ContainerLockGuard::lock_all(&[default_ref]);
        let mut stack = ItemStack::new(&vanilla_items::STONE);
        stack.set(MAX_STACK_SIZE, 99);
        stack.set_count(99);
        let default_remainder = default_slot.safe_insert(&mut default_guard, stack, 99);

        assert!(default_remainder.is_empty());
        assert_eq!(default_slot.get_item(&default_guard).count(), 99);
    }
}
