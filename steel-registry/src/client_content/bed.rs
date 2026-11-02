//! Complete bed state families using extracted vanilla bed definitions.

use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};

use steel_utils::{BlockStateId, Identifier};

use crate::{
    Registry, RegistryExt,
    blocks::{Block, BlockRef},
    items::{Item, ItemRef},
};

use super::{BlockItemDisplay, ClientBlock};

#[cfg(test)]
mod tests;

pub struct BedModel {
    pub properties: Vec<(String, String)>,
    pub displays: Vec<BlockItemDisplay>,
}

pub struct BedRegistration {
    pub key: Identifier,
    pub template: BlockRef,
    pub models: Vec<BedModel>,
    pub item: Item,
    pub item_fallback: ItemRef,
}

#[derive(Debug)]
pub struct ClientBed {
    pub template: BlockRef,
    pub models: Vec<Arc<[BlockItemDisplay]>>,
}

impl Registry {
    /// Registers a complete, ordinary bed: facing, occupancy, both halves, and
    /// one placing item. Core supplies the bed behavior after registry freeze.
    pub fn register_client_bed(&mut self, definition: BedRegistration) -> Result<BlockRef, String> {
        let BedRegistration {
            key,
            template,
            models,
            mut item,
            item_fallback,
        } = definition;
        self.validate_client_item(&item, item_fallback)?;
        if item.block.is_some() {
            return Err("a new bed item must not already place a block".into());
        }
        if !self.client_content.is_vanilla_block(template)
            || !self
                .blocks
                .by_key(&template.key)
                .is_some_and(|block| std::ptr::eq(block, template))
        {
            return Err("the bed template must be a registered vanilla block".into());
        }
        let names: Vec<_> = template
            .properties
            .iter()
            .map(|property| property.get_name())
            .collect();
        if names != ["facing", "occupied", "part"] || template == &crate::vanilla_blocks::STRAW_BED
        {
            return Err(
                "the bed template must expose vanilla facing, occupied, and part states".into(),
            );
        }
        let carrier = self.blocks.get_default_state_id(template);
        self.validate_client_block(&key, carrier, template, &[])?;
        let count = template.state_count();
        if self.blocks.next_state_id.checked_add(count).is_none() {
            return Err("the block-state registry is full".into());
        }
        let base = self.blocks.get_base_state_id(template);
        let empty: Arc<[BlockItemDisplay]> = Arc::from([]);
        let mut state_models: Vec<_> = (0..count).map(|_| Arc::clone(&empty)).collect();
        let mut assigned = vec![false; usize::from(count)];
        for model in models {
            self.validate_client_block(&key, carrier, template, &model.displays)?;
            if self
                .blocks
                .state_id_from_block_defaulted_properties(
                    template,
                    model
                        .properties
                        .iter()
                        .map(|(name, value)| (name.as_str(), value.as_str())),
                )
                .is_none()
            {
                return Err("invalid bed model properties".into());
            }
            let mut names = BTreeSet::new();
            if model.properties.iter().any(|(name, _)| !names.insert(name)) {
                return Err("duplicate bed model property".into());
            }
            let displays: Arc<[BlockItemDisplay]> = model.displays.into();
            for offset in 0..count {
                let state = BlockStateId(base.0 + offset);
                if model.properties.iter().all(|(name, value)| {
                    self.blocks.get_property_str(state, name) == Some(value.as_str())
                }) {
                    let index = usize::from(offset);
                    if assigned[index] {
                        return Err("overlapping bed model selectors".into());
                    }
                    assigned[index] = true;
                    state_models[index] = Arc::clone(&displays);
                }
            }
        }
        // This is an independent definition with the template's complete state
        // family. Its identity, item, loot, and behavior belong to the extension.
        let mut config = template.config.clone();
        config.loot_table = None;
        let block = Box::leak(Box::new(Block {
            key,
            config,
            properties: template.properties,
            default_state_offset: template.default_state_offset,
            suffocating: template.suffocating,
            redstone_conductor: template.redstone_conductor,
            fluid_state: template.fluid_state,
            randomly_ticking: template.randomly_ticking,
            light_properties: template.light_properties,
            collision_shape: template.collision_shape,
            support_shape: template.support_shape,
            outline_shape: template.outline_shape,
            occlusion_shape: template.occlusion_shape,
            interaction_shape: template.interaction_shape,
            visual_shape: template.visual_shape,
            shape_offsets: template.shape_offsets,
            id: OnceLock::new(),
        }));
        let id = self.blocks.register(block);
        item.block = Some(block);
        let item = self.register_client_item(item, item_fallback)?;
        self.client_content.blocks.insert(
            id,
            ClientBlock {
                block,
                carrier,
                material: Some(template),
                server_mining: false,
                displays: Vec::new(),
                self_drop: Some(item),
                horizontal: None,
                state_family: None,
                bed: Some(ClientBed {
                    template,
                    models: state_models,
                }),
            },
        );
        Ok(block)
    }
}
