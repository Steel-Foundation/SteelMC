//! Ordinary server block definitions with an automatically chosen client state.

use crate::{
    Registry, RegistryEntry, RegistryExt,
    blocks::{Block, BlockRef, behavior::OffsetType},
    items::{Item, ItemRef},
};

use super::{BlockItemDisplay, ClientBlock, HorizontalPlacement};

pub struct BlockDefinition {
    pub block: Block,
    pub mining_material: Option<BlockRef>,
    pub preferred_geometry: Option<BlockRef>,
    pub allow_approximation: bool,
    pub horizontal: Option<HorizontalPlacement>,
    pub displays: Vec<BlockItemDisplay>,
}

pub struct PlacingItem {
    pub item: Item,
    pub fallback: ItemRef,
    pub drop_self: bool,
}

impl Registry {
    /// Registers a static block without deriving its server geometry from a
    /// transport state. All fallible validation precedes registry mutation.
    pub fn register_block_definition(
        &mut self,
        definition: BlockDefinition,
        placing_item: Option<PlacingItem>,
    ) -> Result<BlockRef, String> {
        let BlockDefinition {
            mut block,
            mining_material,
            preferred_geometry,
            allow_approximation,
            horizontal,
            displays,
        } = definition;
        self.validate_content_key(&block.key)?;
        if self.blocks.by_key(&block.key).is_some() {
            return Err(format!("block {} is already registered", block.key));
        }
        if block.id.get().is_some()
            || block.state_count() != 1
            || block.config.dynamic_shape
            || block.config.liquid
            || block.config.is_air
            || block.config.offset_type != OffsetType::None
        {
            return Err(
                "this registration requires a new static, non-fluid block with one state".into(),
            );
        }
        if let Some(material) = mining_material
            && (!self.client_content.is_vanilla_block(material)
                || !self
                    .blocks
                    .by_key(&material.key)
                    .is_some_and(|registered| std::ptr::eq(material, registered)))
        {
            return Err("copied tool membership must reference a registered vanilla block".into());
        }
        self.validate_displays(&displays)?;
        if let Some(placing) = &placing_item {
            self.validate_client_item(&placing.item, placing.fallback)?;
            if placing.item.block.is_some() {
                return Err("the placing item must not already belong to a block".into());
            }
        }
        let horizontal = horizontal
            .map(|placement| {
                self.horizontal_definition(
                    &mut block,
                    &displays,
                    preferred_geometry.or(mining_material),
                    allow_approximation,
                    placement,
                )
            })
            .transpose()?;
        if self
            .blocks
            .next_state_id
            .checked_add(block.state_count())
            .is_none()
        {
            return Err("the block-state registry is full".into());
        }
        let carrier = if let Some(facing) = &horizontal {
            facing.states[usize::from(block.default_state_offset)].carrier
        } else {
            self.select_client_state(
                block.get_collision_shape(0),
                block.get_outline_shape(0),
                preferred_geometry.or(mining_material),
                allow_approximation,
            )?
        };
        let block = Box::leak(Box::new(block));
        let id = self.blocks.register(block);
        self.client_content.blocks.insert(
            id,
            ClientBlock {
                block,
                carrier,
                material: mining_material,
                server_mining: true,
                bed: None,
                horizontal,
                state_family: None,
                displays,
                self_drop: None,
            },
        );
        if let Some(PlacingItem {
            mut item,
            fallback,
            drop_self,
        }) = placing_item
        {
            item.block = Some(block);
            let item = self.register_client_item(item, fallback)?;
            if drop_self {
                let Some(model) = self.client_content.blocks.get_mut(&id) else {
                    panic!("new block representation is missing");
                };
                model.self_drop = Some(item);
            }
        }
        Ok(block)
    }

    pub(super) fn validate_displays(&self, displays: &[BlockItemDisplay]) -> Result<(), String> {
        for display in displays {
            if display.item.is_empty()
                || display.item.count() > 99
                || !self.client_content.is_vanilla_item(display.item.item())
                || !self
                    .items
                    .by_id(display.item.item().id())
                    .is_some_and(|item| std::ptr::eq(item, display.item.item()))
                || !display.translation.is_finite()
                || !display.scale.is_finite()
                || display.scale.min_element() <= 0.0
                || !display.rotation.is_finite()
                || (display.rotation.length_squared() - 1.0).abs() > 1.0e-4
            {
                return Err("block displays require a registered vanilla item, finite transforms, positive scales, and unit rotation quaternions".into());
            }
        }
        Ok(())
    }
}
