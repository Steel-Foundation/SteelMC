//! Server-only fluids projected onto vanilla water or lava.
//!
//! A registration creates a source and a flowing fluid, plus a liquid block with
//! the same 16 `level` states as the vanilla liquid it is displayed as. Saves use
//! the custom keys. Clients receive the vanilla liquid state with the same offset,
//! and the fluid it represents, so their movement prediction matches the server.

use std::sync::OnceLock;

use steel_utils::Identifier;

use crate::{
    Registry, RegistryExt,
    blocks::{Block, BlockRef, StateFluidData, StateFluidOverwrite},
    fluid::{Fluid, FluidRef, FluidState},
    items::{Item, ItemRef},
    vanilla_blocks, vanilla_fluids,
};

use super::ClientBlock;

/// Client water colors substituted in the biome cells that contain a fluid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidTint {
    pub water_color: i32,
    pub water_fog_color: i32,
}

/// Vanilla spreading parameters that the host's flowing-fluid algorithm uses.
///
/// Vanilla hardcodes these in each `FlowingFluid` subclass; registered fluids
/// carry them as data so the host can run the same algorithm for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidSpread {
    pub tick_delay: u16,
    pub drop_off: u8,
    pub slope_find_distance: u8,
    pub converts_to_source: bool,
}

/// One registered fluid pair and its vanilla representation.
#[derive(Debug)]
pub struct ClientFluid {
    pub source: FluidRef,
    pub flowing: FluidRef,
    pub block: BlockRef,
    /// The vanilla source fluid clients see (`minecraft:water` or `minecraft:lava`).
    pub client_source: FluidRef,
    /// The vanilla flowing variant of `client_source`.
    pub client_flowing: FluidRef,
    pub tint: Option<FluidTint>,
    /// Dense index among tinting fluids, assigned in registration order.
    pub tint_index: Option<u16>,
    pub spread: FluidSpread,
}

impl ClientFluid {
    /// The vanilla fluid variant clients see for one of this pair's variants.
    #[must_use]
    pub const fn client_variant(&self, fluid: FluidRef) -> FluidRef {
        if fluid.is_source {
            self.client_source
        } else {
            self.client_flowing
        }
    }
}

/// Everything needed to register one fluid pair.
pub struct FluidRegistration {
    pub source_key: Identifier,
    pub flowing_key: Identifier,
    pub block_key: Identifier,
    /// `minecraft:water` or `minecraft:lava`.
    pub client_source: FluidRef,
    pub tint: Option<FluidTint>,
    pub spread: FluidSpread,
    pub explosion_resistance: f32,
    pub randomly_ticking: bool,
    /// The filled bucket item and its vanilla representation.
    pub bucket: Option<(Item, ItemRef)>,
}

const LIQUID_STATE_COUNT: u16 = 16;
const MAX_COLOR: i32 = 0xFF_FFFF;

impl Registry {
    /// Registers a fluid pair, its liquid block, and an optional bucket.
    ///
    /// Everything is validated before anything is inserted, so a rejected
    /// definition leaves no partial entries behind.
    pub fn register_client_fluid(
        &mut self,
        definition: FluidRegistration,
    ) -> Result<&'static ClientFluid, String> {
        self.validate_client_fluid(&definition)?;
        let FluidRegistration {
            source_key,
            flowing_key,
            block_key,
            client_source,
            tint,
            spread,
            explosion_resistance,
            randomly_ticking,
            bucket,
        } = definition;
        let (template, client_flowing): (BlockRef, FluidRef) =
            if client_source == &vanilla_fluids::WATER {
                (&vanilla_blocks::WATER, &vanilla_fluids::FLOWING_WATER)
            } else {
                (&vanilla_blocks::LAVA, &vanilla_fluids::FLOWING_LAVA)
            };
        let bucket_key = bucket.as_ref().map_or_else(
            || Identifier::vanilla_static("air"),
            |(item, _)| item.key.clone(),
        );
        let fluid = |key: &Identifier, is_source: bool| -> FluidRef {
            Box::leak(Box::new(Fluid {
                key: key.clone(),
                is_empty: false,
                is_source,
                is_randomly_ticking: randomly_ticking,
                block: block_key.clone(),
                bucket_item: bucket_key.clone(),
                source_fluid: Some(source_key.clone()),
                flowing_fluid: Some(flowing_key.clone()),
                tick_delay: u32::from(spread.tick_delay),
                explosion_resistance,
            }))
        };
        let source = fluid(&source_key, true);
        let flowing = fluid(&flowing_key, false);
        let source_id = self.fluids.register(source);
        let flowing_id = self.fluids.register(flowing);

        let mut config = template.config.clone();
        config.loot_table = None;
        config.explosion_resistance = explosion_resistance;
        let block: BlockRef = Box::leak(Box::new(Block {
            key: block_key,
            config,
            properties: template.properties,
            default_state_offset: template.default_state_offset,
            suffocating: template.suffocating,
            redstone_conductor: template.redstone_conductor,
            fluid_state: liquid_fluid_states(source, flowing),
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
        let block_id = self.blocks.register(block);
        self.client_content.blocks.insert(
            block_id,
            ClientBlock {
                block,
                carrier: self.blocks.get_default_state_id(template),
                material: None,
                server_mining: false,
                bed: None,
                horizontal: None,
                state_family: Some(template),
                displays: Vec::new(),
                self_drop: None,
            },
        );
        if let Some((item, fallback)) = bucket {
            // Validated with the rest of the definition above.
            self.register_client_item(item, fallback)?;
        }

        let tint_index = tint.map(|_| {
            let index = self.client_content.tinting_fluid_count;
            self.client_content.tinting_fluid_count += 1;
            index
        });
        let entry: &'static ClientFluid = Box::leak(Box::new(ClientFluid {
            source,
            flowing,
            block,
            client_source,
            client_flowing,
            tint,
            tint_index,
            spread,
        }));
        self.client_content.fluids.insert(source_id, entry);
        self.client_content.fluids.insert(flowing_id, entry);
        self.client_content.fluid_entries.push(entry);
        Ok(entry)
    }

    fn validate_client_fluid(&self, definition: &FluidRegistration) -> Result<(), String> {
        for key in [&definition.source_key, &definition.flowing_key] {
            self.validate_content_key(key)?;
            if self.fluids.by_key(key).is_some() {
                return Err(format!("fluid {key} is already registered"));
            }
        }
        if definition.source_key == definition.flowing_key {
            return Err("the source and flowing fluids need distinct keys".into());
        }
        self.validate_content_key(&definition.block_key)?;
        if self.blocks.by_key(&definition.block_key).is_some() {
            return Err(format!(
                "block {} is already registered",
                definition.block_key
            ));
        }
        let client = definition.client_source;
        let is_water = client == &vanilla_fluids::WATER;
        if !is_water && client != &vanilla_fluids::LAVA {
            return Err("a fluid must be displayed as minecraft:water or minecraft:lava".into());
        }
        if let Some(tint) = definition.tint {
            if !is_water {
                return Err("only fluids displayed as water can carry a water tint".into());
            }
            for color in [tint.water_color, tint.water_fog_color] {
                if !(0..=MAX_COLOR).contains(&color) {
                    return Err(format!("tint color {color:#x} is not a 24-bit RGB value"));
                }
            }
        }
        let spread = definition.spread;
        if !(1..=1200).contains(&spread.tick_delay) {
            return Err("tick_delay must be between 1 and 1200".into());
        }
        if !(1..=7).contains(&spread.drop_off) {
            return Err("drop_off must be between 1 and 7".into());
        }
        if spread.slope_find_distance > 8 {
            return Err("slope_find_distance must be between 0 and 8".into());
        }
        if !definition.explosion_resistance.is_finite() || definition.explosion_resistance < 0.0 {
            return Err("explosion_resistance must be a finite, non-negative number".into());
        }
        if self
            .blocks
            .next_state_id
            .checked_add(LIQUID_STATE_COUNT)
            .is_none()
        {
            return Err("the block-state registry is full".into());
        }
        if let Some((item, fallback)) = &definition.bucket {
            self.validate_client_item(item, fallback)?;
            if item.block.is_some() {
                return Err("a bucket item must not place a block".into());
            }
        }
        Ok(())
    }
}

/// Fluid states for the 16 `level` values, laid out like vanilla's liquid blocks:
/// level 0 is the source, 1–7 flow with amount `8 - level`, and 8–15 fall.
fn liquid_fluid_states(source: FluidRef, flowing: FluidRef) -> StateFluidData {
    let mut overwrites = Vec::with_capacity(8);
    overwrites.push(StateFluidOverwrite::new(0, FluidState::source(source)));
    for level in 1..=7_u8 {
        overwrites.push(StateFluidOverwrite::new(
            u16::from(level),
            FluidState::flowing(flowing, 8 - level, false),
        ));
    }
    StateFluidData::new(
        FluidState::flowing(flowing, 8, true),
        Box::leak(overwrites.into_boxed_slice()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_components::DataComponentMap;
    use crate::{RegistryEntry as _, vanilla_items};

    fn sludge() -> FluidRegistration {
        FluidRegistration {
            source_key: Identifier::new("fluid_test", "sludge"),
            flowing_key: Identifier::new("fluid_test", "sludge_flowing"),
            block_key: Identifier::new("fluid_test", "sludge"),
            client_source: &vanilla_fluids::WATER,
            tint: Some(FluidTint {
                water_color: 0x4C_BB17,
                water_fog_color: 0x2E_5E0E,
            }),
            spread: FluidSpread {
                tick_delay: 15,
                drop_off: 2,
                slope_find_distance: 4,
                converts_to_source: false,
            },
            explosion_resistance: 100.0,
            randomly_ticking: true,
            bucket: None,
        }
    }

    fn bucket(key: &'static str) -> (Item, ItemRef) {
        (
            Item {
                key: Identifier::new("fluid_test", key),
                components: DataComponentMap::new(),
                block: None,
                craft_remainder: None,
                id: OnceLock::new(),
            },
            &vanilla_items::WATER_BUCKET,
        )
    }

    #[test]
    fn registered_pair_links_variants_block_and_bucket() -> Result<(), String> {
        let mut registry = Registry::new_vanilla();
        let mut definition = sludge();
        definition.bucket = Some(bucket("sludge_bucket"));
        let entry = registry.register_client_fluid(definition)?;

        assert!(entry.source.is_source && !entry.flowing.is_source);
        assert_eq!(entry.source.flowing_fluid, Some(entry.flowing.key.clone()));
        assert_eq!(entry.flowing.source_fluid, Some(entry.source.key.clone()));
        assert!(entry.source.is_randomly_ticking);
        assert_eq!(entry.source.block, entry.block.key);
        assert_eq!(
            entry.source.bucket_item,
            Identifier::new("fluid_test", "sludge_bucket")
        );
        assert_eq!(entry.tint_index, Some(0));
        assert!(registry.items.by_key(&entry.source.bucket_item).is_some());
        assert!(std::ptr::eq(
            registry
                .blocks
                .by_key(&entry.block.key)
                .ok_or("block missing")?,
            entry.block
        ));
        assert_eq!(entry.client_variant(entry.source), &vanilla_fluids::WATER);
        assert_eq!(
            entry.client_variant(entry.flowing).key,
            vanilla_fluids::FLOWING_WATER.key
        );
        Ok(())
    }

    #[test]
    fn every_liquid_level_decodes_to_the_custom_variants() -> Result<(), String> {
        let mut registry = Registry::new_vanilla();
        let entry = registry.register_client_fluid(sludge())?;
        let block = entry.block;
        let water = &vanilla_blocks::WATER;
        assert_eq!(block.state_count(), water.state_count());
        assert_eq!(block.properties.len(), water.properties.len());
        for offset in 0..block.state_count() {
            let custom = block.fluid_state.value(offset);
            let vanilla = water.fluid_state.value(offset);
            assert_eq!(custom.amount, vanilla.amount, "offset {offset}");
            assert_eq!(custom.falling, vanilla.falling, "offset {offset}");
            let expected = if vanilla.fluid_id.is_source {
                entry.source
            } else {
                entry.flowing
            };
            assert!(std::ptr::eq(custom.fluid_id, expected), "offset {offset}");
        }
        let entry_block = registry
            .client_content
            .blocks
            .values()
            .find(|client| std::ptr::eq(client.block, block))
            .ok_or("client block missing")?;
        assert_eq!(
            entry_block.state_family.map(|t| t.key.clone()),
            Some(water.key.clone())
        );
        Ok(())
    }

    type Mutation = fn(&mut FluidRegistration);

    fn reject(registry: &mut Registry, name: &str, mutate: Mutation) {
        let counts = (
            registry.fluids.len(),
            registry.blocks.len(),
            registry.items.len(),
        );
        let mut definition = sludge();
        mutate(&mut definition);
        assert!(
            registry.register_client_fluid(definition).is_err(),
            "{name}"
        );
        assert_eq!(
            (
                registry.fluids.len(),
                registry.blocks.len(),
                registry.items.len()
            ),
            counts,
            "{name} left partial entries"
        );
    }

    #[test]
    fn invalid_definitions_are_rejected_without_partial_entries() {
        let cases: [(&str, Mutation); 11] = [
            ("vanilla namespace", |d| {
                d.source_key = Identifier::vanilla_static("sludge");
            }),
            ("same keys", |d| d.flowing_key = d.source_key.clone()),
            ("vanilla block", |d| {
                d.block_key = Identifier::vanilla_static("water");
            }),
            ("lava tint", |d| d.client_source = &vanilla_fluids::LAVA),
            ("not a source", |d| {
                d.client_source = &vanilla_fluids::FLOWING_WATER;
            }),
            ("color", |d| {
                d.tint = Some(FluidTint {
                    water_color: 0x100_0000,
                    water_fog_color: 0,
                });
            }),
            ("tick delay", |d| d.spread.tick_delay = 0),
            ("drop off", |d| d.spread.drop_off = 8),
            ("slope", |d| d.spread.slope_find_distance = 9),
            ("resistance", |d| d.explosion_resistance = f32::NAN),
            ("vanilla bucket", |d| {
                let (mut item, fallback) = bucket("sludge_bucket");
                item.key = Identifier::vanilla_static("water_bucket");
                d.bucket = Some((item, fallback));
            }),
        ];
        for (name, mutate) in cases {
            reject(&mut Registry::new_vanilla(), name, mutate);
        }
    }

    #[test]
    fn a_taken_block_key_is_rejected() -> Result<(), String> {
        let mut registry = Registry::new_vanilla();
        registry.register_client_fluid(FluidRegistration {
            source_key: Identifier::new("fluid_test", "other"),
            flowing_key: Identifier::new("fluid_test", "other_flowing"),
            ..sludge()
        })?;
        reject(&mut registry, "taken block", |_| {});
        Ok(())
    }

    #[test]
    fn duplicate_fluid_keys_are_rejected() -> Result<(), String> {
        let mut registry = Registry::new_vanilla();
        registry.register_client_fluid(sludge())?;
        let mut again = sludge();
        again.block_key = Identifier::new("fluid_test", "other_block");
        assert!(registry.register_client_fluid(again).is_err());
        assert_eq!(registry.client_content.fluids().len(), 1);
        assert!(
            registry
                .fluids
                .by_key(&Identifier::new("fluid_test", "sludge"))
                .is_some_and(|f| f.key().path == "sludge")
        );
        Ok(())
    }
}
