use core::sync::atomic::Ordering;
use std::sync::{Arc, atomic::AtomicI32};

use crate::inventory::{
    container::{Container, ResultContainer, SimpleContainer},
    lock::{ContainerId, ContainerLockGuard, ContainerRef},
    slots::result_handler::ResultHandler,
};
use steel_registry::{
    REGISTRY, RegistryHolder,
    RegistryHolderSet::{Direct, Tag},
    TaggedRegistryExt,
    banner_pattern::BannerPattern,
    data_components::{
        BannerPatternLayer, BannerPatternLayers,
        vanilla_components::{BANNER_PATTERNS, DYE, DyeColor, PROVIDES_BANNER_PATTERNS},
    },
    item_stack::ItemStack,
    vanilla_banner_pattern_tags::BannerPatternTag,
};
use steel_utils::locks::Shared;

/// Handler for Loom
#[derive(Clone)]
pub struct LoomHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    button_id: Arc<AtomicI32>,
    buttons_len: Arc<AtomicI32>,
}

const BANNER_SLOT: usize = 0;
const DYE_SLOT: usize = 1;
const PATTERN_SLOT: usize = 2;

impl LoomHandler {
    /// Creates a new Loom Handler
    pub const fn new(
        input_container: Shared<SimpleContainer>,
        result_container: Shared<ResultContainer>,
        button_id: Arc<AtomicI32>,
        buttons_len: Arc<AtomicI32>,
    ) -> Self {
        Self {
            input_container,
            result_container,
            button_id,
            buttons_len,
        }
    }

    fn get_selectable_patterns(&self, pattern: &ItemStack) -> Vec<&'static BannerPattern> {
        if pattern.is_empty() {
            REGISTRY
                .banner_patterns
                .get_tag(&BannerPatternTag::NO_ITEM_REQUIRED)
                .unwrap_or_default()
        } else {
            match pattern.get(PROVIDES_BANNER_PATTERNS) {
                Some(Tag(id)) => REGISTRY.banner_patterns.get_tag(&id).unwrap_or_default(),
                Some(Direct(v)) => v.to_vec(),
                None => vec![],
            }
        }
    }

    /// The `ContainerId` of the input container
    #[must_use]
    pub fn input_id(&self) -> ContainerId {
        ContainerId::from_arc(&self.input_container)
    }

    /// The `ContainerId` of the result container
    #[must_use]
    pub fn result_id(&self) -> ContainerId {
        ContainerId::from_arc(&self.result_container)
    }

    fn get_result(&self, guard: &ContainerLockGuard) -> Option<ItemStack> {
        let input_container = guard
            .get_typed::<SimpleContainer>(self.input_id())
            .expect("input container not locked");

        if input_container.items().get(BANNER_SLOT).unwrap().is_empty()
            || input_container.items().get(DYE_SLOT).unwrap().is_empty()
        {
            return None;
        }
        let patterns =
            self.get_selectable_patterns(input_container.items().get(PATTERN_SLOT).expect(""));
        if patterns.len() == 1 {
            self.button_id.store(0, Ordering::Relaxed);
        }

        let button_id = self.button_id.load(Ordering::Relaxed);
        if button_id < 0 {
            return None;
        }
        if patterns.len() == 0 || button_id as usize >= patterns.len() {
            self.button_id.store(-1, Ordering::Relaxed);
            return None;
        }
        let pattern = patterns.get(self.button_id.load(Ordering::Relaxed) as usize);
        let color = input_container
            .items()
            .get(DYE_SLOT)
            .unwrap()
            .get::<DyeColor>(DYE);

        let mut result = input_container.items().get(BANNER_SLOT).unwrap().clone();
        let mut layers = result
            .get_or_default::<BannerPatternLayers>(BANNER_PATTERNS, BannerPatternLayers::empty())
            .layers()
            .to_vec();
        layers.push(BannerPatternLayer::new(
            RegistryHolder::Reference(pattern.unwrap()),
            *color.unwrap(),
        ));
        result.count = 1;
        result.set(BANNER_PATTERNS, BannerPatternLayers::new(layers));

        return Some(result);
    }
}

impl ResultHandler for LoomHandler {
    fn result_container(&self) -> ContainerRef {
        ContainerRef::from(self.result_container.clone())
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![ContainerRef::from(self.input_container.clone())]
    }

    fn update_result(&self, guard: &mut ContainerLockGuard) {
        // Temp store large number for buttons
        // TODO: Remove but cache selectable_patterns
        self.buttons_len.store(64, Ordering::Relaxed);

        let result = self.get_result(guard).unwrap_or_default();
        let result_container = guard
            .get_typed_mut::<ResultContainer>(self.result_id())
            .expect("result container not locked");
        result_container.set_item(0, result);
        result_container.set_changed();
    }

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        _player: &crate::inventory::prelude::Player,
    ) -> Option<ItemStack> {
        let input_container = guard
            .get_typed_mut::<SimpleContainer>(self.input_id())
            .expect("input container not locked");
        input_container.get_item_mut(BANNER_SLOT).shrink(1);
        input_container.get_item_mut(DYE_SLOT).shrink(1);

        self.update_result(guard);
        return None;
    }

    fn is_result_valid(
        &self,
        guard: &ContainerLockGuard,
        _player: &crate::inventory::prelude::Player,
    ) -> bool {
        let result = self.get_result(guard).unwrap_or_default();
        let result_container = guard
            .get_typed::<ResultContainer>(self.result_id())
            .expect("result container not locked");
        ItemStack::matches(&result, result_container.get_item(0))
    }
}
