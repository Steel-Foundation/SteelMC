//! Typed content construction on the host side of the native boundary.

mod definition;
#[cfg(test)]
mod tests;

use std::sync::{Arc, OnceLock};

use glam::{Quat, Vec3};
use steel_registry::{
    Registry, RegistryExt,
    client_content::{self, BedModel, BedRegistration, BlockItemDisplay, PlacingItem},
    data_components::{
        DataComponentMap, DataComponentPatch,
        vanilla_components::{ITEM_MODEL, ITEM_NAME, MAX_STACK_SIZE, PROFILE},
    },
    game_profile::ProfileProperty,
    item_stack::ItemStack,
    items::{Item, ItemRef},
    resolvable_profile::{PartialProfile, PlayerSkinPatch, ResolvableProfile},
    vanilla_items,
};
use steel_utils::{Identifier, UuidExt};
use text_components::TextComponent;
use uuid::Uuid;

use super::HookRegistration;
use crate::{
    ApiResult, BlockState,
    content::{
        BedDefinition, BlockDefinition, BlockItemDefinition, ContentRegistry, HeadProfile,
        ItemAppearance, ItemDefinition, ItemDisplay, SlabMaterial,
    },
};
use stabby::vec::Vec as ApiVec;

pub(crate) struct ContentRegistration<'a> {
    pub registry: &'a mut Registry,
    pub config: &'a str,
}

pub(crate) fn subscribe(
    hooks: &mut HookRegistration<'_>,
    callback: extern "C" fn(ContentRegistry<'_>) -> ApiResult<()>,
) -> Result<(), String> {
    hooks.extension.begin_registration();
    let extension = Arc::clone(hooks.extension);
    let result = client_content::on_registration(move |registry| {
        extension.run_registration("content", || {
            let mut registration = ContentRegistration {
                registry,
                config: &extension.config,
            };
            callback(ContentRegistry::from_host(&mut registration))
        });
    });
    if result.is_err() {
        hooks.extension.cancel_registration();
    }
    result
}

pub(crate) fn register_item(
    registry: &mut Registry,
    definition: ItemDefinition,
) -> Result<(), String> {
    let (item, fallback) = prepare_item(registry, definition)?;
    registry.register_client_item(item, fallback)?;
    Ok(())
}

fn prepare_item(
    registry: &Registry,
    definition: ItemDefinition,
) -> Result<(Item, ItemRef), String> {
    if !(1..=99).contains(&definition.max_stack_size) {
        return Err("item maximum stack size must be between 1 and 99".into());
    }
    let key = definition
        .key
        .as_str()
        .parse::<Identifier>()
        .map_err(ToString::to_string)?;
    let (fallback, mut components) = appearance(registry, &definition.appearance)?;
    components.set(
        ITEM_NAME,
        Some(TextComponent::from(definition.name.as_str().to_owned())),
    );
    components.set(MAX_STACK_SIZE, Some(definition.max_stack_size));
    Ok((
        Item {
            key,
            components,
            block: None,
            craft_remainder: None,
            id: OnceLock::new(),
        },
        fallback,
    ))
}

pub(crate) fn register_block(
    registry: &mut Registry,
    definition: BlockDefinition,
) -> Result<BlockState, String> {
    let definition = definition::prepare(registry, definition)?;
    let block = registry.register_block_definition(definition, None)?;
    Ok(BlockState {
        id: registry.blocks.get_default_state_id(block).0,
    })
}

pub(crate) fn register_block_item(
    registry: &mut Registry,
    definition: BlockItemDefinition,
) -> Result<BlockState, String> {
    let block = definition::prepare(registry, definition.block)?;
    let (item, fallback) = prepare_item(registry, definition.item)?;
    let block = registry.register_block_definition(
        block,
        Some(PlacingItem {
            item,
            fallback,
            drop_self: definition.drop_self,
        }),
    )?;
    Ok(BlockState {
        id: registry.blocks.get_default_state_id(block).0,
    })
}

pub(crate) fn slab_materials(registry: &Registry) -> ApiVec<SlabMaterial> {
    registry
        .blocks
        .iter()
        .filter_map(|(_, block)| {
            if !registry.client_content.is_vanilla_block(block)
                || registry
                    .blocks
                    .state_id_from_block_defaulted_properties(
                        block,
                        [("type", "double"), ("waterlogged", "false")],
                    )
                    .is_none()
            {
                return None;
            }
            let item = registry.items.by_block(block);
            if item == &*vanilla_items::AIR {
                return None;
            }
            Some(SlabMaterial {
                block: block.key.to_string().into(),
                item: item.key.to_string().into(),
            })
        })
        .collect()
}

fn appearance(
    registry: &Registry,
    appearance: &ItemAppearance,
) -> Result<(ItemRef, DataComponentMap), String> {
    let key = appearance
        .item
        .as_str()
        .parse::<Identifier>()
        .map_err(ToString::to_string)?;
    let item = registry
        .items
        .by_key(&key)
        .ok_or_else(|| format!("unknown client item {key}"))?;
    if !registry.client_content.is_vanilla_item(item) || item == &*vanilla_items::AIR {
        return Err("the client appearance must use a nonempty vanilla item".into());
    }
    let mut components = DataComponentMap::common_item_components();
    components.set(ITEM_MODEL, item.components.get(ITEM_MODEL));
    if let Some(model) = appearance.model.as_ref() {
        components.set(
            ITEM_MODEL,
            Some(
                model
                    .as_str()
                    .parse::<Identifier>()
                    .map_err(ToString::to_string)?,
            ),
        );
    }
    if let Some(profile) = appearance.profile.as_ref() {
        if item != &*vanilla_items::PLAYER_HEAD {
            return Err("a head profile requires minecraft:player_head as its client item".into());
        }
        components.set(PROFILE, Some(head_profile(profile)?));
    }
    Ok((item, components))
}

fn head_profile(profile: &HeadProfile) -> Result<ResolvableProfile, String> {
    let id = Uuid::from_int_array(&profile.id).ok_or_else(|| "invalid profile UUID".to_owned())?;
    let property =
        ProfileProperty::new("textures".into(), profile.texture.as_str().to_owned(), None)
            .map_err(|error| error.to_string())?;
    let profile =
        PartialProfile::new(None, Some(id), vec![property]).map_err(|error| error.to_string())?;
    Ok(ResolvableProfile::static_partial(
        profile,
        PlayerSkinPatch::default(),
    ))
}

fn prepare_displays(
    registry: &Registry,
    displays: &[ItemDisplay],
) -> Result<Vec<BlockItemDisplay>, String> {
    displays
        .iter()
        .map(|display| {
            let (item, components) = appearance(registry, &display.appearance)?;
            // Per-display appearance is a stack patch over the actual vanilla item.
            let mut patch = DataComponentPatch::new();
            if let Some(model) = components.get(ITEM_MODEL) {
                patch.set(ITEM_MODEL, model);
            }
            if let Some(profile) = components.get(PROFILE) {
                patch.set(PROFILE, profile);
            }
            Ok(BlockItemDisplay {
                item: ItemStack::with_count_and_patch(item, 1, patch),
                translation: Vec3::from_array(display.translation),
                scale: Vec3::from_array(display.scale),
                rotation: Quat::from_array(display.rotation),
            })
        })
        .collect::<Result<Vec<_>, String>>()
}

pub(crate) fn register_bed(
    registry: &mut Registry,
    definition: BedDefinition,
) -> Result<BlockState, String> {
    let key = definition
        .key
        .as_str()
        .parse::<Identifier>()
        .map_err(ToString::to_string)?;
    let template_key = definition
        .template
        .as_str()
        .parse::<Identifier>()
        .map_err(ToString::to_string)?;
    let template = registry
        .blocks
        .by_key(&template_key)
        .ok_or("unknown bed template")?;
    let (item, item_fallback) = prepare_item(registry, definition.item)?;
    let models = definition
        .models
        .iter()
        .map(|model| {
            Ok(BedModel {
                properties: model
                    .properties
                    .iter()
                    .map(|property| {
                        (
                            property.name.as_str().to_owned(),
                            property.value.as_str().to_owned(),
                        )
                    })
                    .collect(),
                displays: prepare_displays(registry, model.displays.as_slice())?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let block = registry.register_client_bed(BedRegistration {
        key,
        template,
        models,
        item,
        item_fallback,
    })?;
    Ok(BlockState {
        id: registry.blocks.get_default_state_id(block).0,
    })
}
