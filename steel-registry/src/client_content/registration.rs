use std::sync::OnceLock;

use steel_utils::{BlockStateId, Identifier, locks::SyncMutex};

use crate::{
    Registry, RegistryEntry, RegistryExt,
    blocks::{Block, BlockRef, StateBooleanData, StateFluidData, state_data::BlockStateData},
    items::{Item, ItemRef},
    loot_table::LootTable,
};

use super::{BlockItemDisplay, ClientBlock};

type RegistrationHook = Box<dyn FnOnce(&mut Registry) + Send>;
static REGISTRATION_HOOKS: SyncMutex<Option<Vec<RegistrationHook>>> =
    SyncMutex::new(Some(Vec::new()));

/// Independent server definitions registered as one placeable block item.
pub struct BlockItemRegistration {
    pub key: Identifier,
    pub carrier: BlockStateId,
    pub material: BlockRef,
    pub displays: Vec<BlockItemDisplay>,
    pub item: Item,
    pub item_fallback: ItemRef,
    pub drop_self: bool,
}

/// Installs content before freezing. Subscribe before global bootstrap.
pub fn on_registration(hook: impl FnOnce(&mut Registry) + Send + 'static) -> Result<(), String> {
    let mut hooks = REGISTRATION_HOOKS.lock();
    let Some(hooks) = hooks.as_mut() else {
        return Err("content registration has already finished".into());
    };
    hooks.push(Box::new(hook));
    Ok(())
}

pub(crate) fn run_registration_hooks(registry: &mut Registry) {
    let hooks = REGISTRATION_HOOKS.lock().take();
    if let Some(hooks) = hooks {
        for hook in hooks {
            hook(registry);
        }
    }
}

impl Registry {
    /// Registers an independent server item with a vanilla client representation.
    /// The item's default components describe the appearance, including its model.
    pub fn register_client_item(
        &mut self,
        item: Item,
        fallback: ItemRef,
    ) -> Result<ItemRef, String> {
        self.validate_client_item(&item, fallback)?;
        let item = Box::leak(Box::new(item));
        let id = self.items.register(item);
        if let Some(block) = item.block {
            self.items.register_block_item(block, item);
        }
        self.client_content.items.insert(id, fallback);
        Ok(item)
    }

    pub(super) fn validate_client_item(
        &self,
        item: &Item,
        fallback: ItemRef,
    ) -> Result<(), String> {
        self.validate_content_key(&item.key)?;
        if self.items.by_key(&item.key).is_some() {
            return Err(format!("item {} is already registered", item.key));
        }
        if item.id.get().is_some() {
            return Err("a new item must not have a cached registry ID".into());
        }
        if !self.client_content.is_vanilla_item(fallback)
            || !self
                .items
                .by_id(fallback.id())
                .is_some_and(|registered| std::ptr::eq(registered, fallback))
        {
            return Err("the item representation must be a registered vanilla item".into());
        }
        if let Some(block) = item.block
            && (!self
                .blocks
                .by_key(&block.key)
                .is_some_and(|registered| std::ptr::eq(registered, block))
                || self.items.by_block(block) != &*crate::vanilla_items::AIR)
        {
            return Err("the placed block must be registered and have no associated item".into());
        }
        Ok(())
    }

    /// Validates the item before committing the block, so duplicate or invalid
    /// item registrations cannot leave an orphan block behind.
    pub fn register_client_block_item(
        &mut self,
        definition: BlockItemRegistration,
    ) -> Result<BlockRef, String> {
        let BlockItemRegistration {
            key,
            carrier,
            material,
            displays,
            mut item,
            item_fallback,
            drop_self,
        } = definition;
        self.validate_client_item(&item, item_fallback)?;
        if item.block.is_some() {
            return Err("the block item's block is supplied by this registration".into());
        }
        self.validate_client_block(&key, carrier, material, &displays)?;
        let block = self.register_client_block(key, carrier, material, None, displays)?;
        item.block = Some(block);
        // The item namespace, duplicate check, and fallback were validated above;
        // registering the block changes neither the item registry nor its fallback.
        let item = self.register_client_item(item, item_fallback)?;
        if drop_self {
            let Some(entry) = self.client_content.blocks.get_mut(&block.id()) else {
                panic!("newly registered client block is absent from its representation map");
            };
            entry.self_drop = Some(item);
        }
        Ok(block)
    }

    /// Registers one fixed vanilla geometry with an independent mining material.
    /// Dynamic behavior remains independent of both vanilla blocks.
    pub fn register_client_block(
        &mut self,
        key: Identifier,
        carrier: BlockStateId,
        material: BlockRef,
        loot_table: Option<LootTable>,
        displays: Vec<BlockItemDisplay>,
    ) -> Result<BlockRef, String> {
        let (geometry, offset) = self.validate_client_block(&key, carrier, material, &displays)?;
        if let Some(table) = &loot_table
            && self.loot_tables.by_key(&table.key).is_some()
        {
            return Err(format!("loot table {} is already registered", table.key));
        }
        let loot_table = loot_table.map(|table| {
            let table = Box::leak(Box::new(table));
            self.loot_tables.register(table);
            &*table
        });
        let block = Self::client_block_definition(key, geometry, offset, material, loot_table);
        let block = Box::leak(Box::new(block));
        let id = self.blocks.register(block);
        self.client_content.blocks.insert(
            id,
            ClientBlock {
                block,
                carrier,
                material: Some(material),
                server_mining: geometry != material,
                bed: None,
                horizontal: None,
                state_family: None,
                displays,
                self_drop: None,
            },
        );
        Ok(block)
    }

    pub(super) fn validate_client_block(
        &self,
        key: &Identifier,
        carrier: BlockStateId,
        material: BlockRef,
        displays: &[BlockItemDisplay],
    ) -> Result<(BlockRef, u16), String> {
        self.validate_content_key(key)?;
        if self.blocks.by_key(key).is_some() {
            return Err(format!("block {key} is already registered"));
        }
        let geometry = self
            .blocks
            .by_state_id(carrier)
            .filter(|block| self.client_content.is_vanilla_block(block))
            .ok_or("the carrier must be a registered vanilla block state")?;
        let offset = carrier.0 - self.blocks.get_base_state_id(geometry).0;
        if geometry.config.dynamic_shape
            || self.block_entity_types.has_block_entity(geometry)
            || geometry.config.liquid
            || geometry.config.is_air
            || !geometry.fluid_state.value(offset).is_empty()
            || geometry.get_outline_shape(offset).is_empty()
        {
            return Err("the carrier must have static, targetable vanilla geometry without a block entity or fluid".into());
        }
        if !self.client_content.is_vanilla_block(material)
            || !self
                .blocks
                .by_id(material.id())
                .is_some_and(|registered| std::ptr::eq(registered, material))
            || material.config.destroy_time < 0.0
            || material.config.is_air
            || material.config.liquid
        {
            return Err("the mining material must be a registered, mineable vanilla block".into());
        }
        if self.blocks.next_state_id == u16::MAX {
            return Err("the block-state registry is full".into());
        }
        self.validate_displays(displays)?;
        Ok((geometry, offset))
    }

    fn client_block_definition(
        key: Identifier,
        geometry: BlockRef,
        offset: u16,
        material: BlockRef,
        loot_table: Option<crate::loot_table::LootTableRef>,
    ) -> Block {
        // Mining and surface properties belong to the material; the selected
        // carrier supplies geometry, positional offsets, and lighting.
        let mut config = material.config.clone();
        config.loot_table = loot_table;
        config.has_collision = geometry.config.has_collision;
        config.can_occlude = geometry.config.can_occlude;
        config.force_solid_off = geometry.config.force_solid_off;
        config.force_solid_on = geometry.config.force_solid_on;
        config.dynamic_shape = false;
        config.offset_type = geometry.config.offset_type;
        config.max_horizontal_offset = geometry.config.max_horizontal_offset;
        config.max_vertical_offset = geometry.config.max_vertical_offset;
        config.is_randomly_ticking = false;
        config.replaceable = false;
        Block {
            key,
            config,
            properties: &[],
            default_state_offset: 0,
            suffocating: StateBooleanData::new(geometry.suffocating.value(offset), &[]),
            redstone_conductor: StateBooleanData::new(
                geometry.redstone_conductor.value(offset),
                &[],
            ),
            fluid_state: StateFluidData::EMPTY,
            randomly_ticking: StateBooleanData::FALSE,
            light_properties: BlockStateData::Fixed(geometry.get_light_properties(offset)),
            collision_shape: BlockStateData::Fixed(geometry.get_collision_shape(offset)),
            support_shape: BlockStateData::Fixed(geometry.get_support_shape(offset)),
            outline_shape: BlockStateData::Fixed(geometry.get_outline_shape(offset)),
            occlusion_shape: BlockStateData::Fixed(geometry.get_occlusion_shape(offset)),
            interaction_shape: BlockStateData::Fixed(geometry.get_interaction_shape(offset)),
            visual_shape: BlockStateData::Fixed(geometry.get_visual_shape(offset)),
            shape_offsets: geometry.shape_offsets,
            id: OnceLock::new(),
        }
    }

    pub(super) fn validate_content_key(&self, key: &Identifier) -> Result<(), String> {
        if self.client_content.frozen {
            return Err("content registration has already finished".into());
        }
        if key.namespace == Identifier::VANILLA_NAMESPACE {
            return Err("custom content must use its owner's namespace".into());
        }
        if !Identifier::validate(&key.namespace, &key.path) {
            return Err(format!("invalid content key {key}"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
