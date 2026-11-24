use quote::format_ident;
use serde::Deserialize;

use super::super::{
    Compiler, TokenStream, ident, identifier,
    model::{CompileModifier, Condition, Holder, HolderSet, yes},
    quote,
};

const fn default_zoom() -> i8 {
    2
}
const fn default_radius() -> i32 {
    50
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplorationMap {
    pub destination: HolderSet,
    pub decoration: Option<String>,
    #[serde(default = "default_zoom")]
    pub zoom: i8,
    #[serde(default = "default_radius")]
    pub search_radius: i32,
    #[serde(default = "yes")]
    pub skip_existing_chunks: bool,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for ExplorationMap {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream {
        assert!(
            compiler.resumable,
            "exploration-map tables require resumable code generation"
        );
        let destinations = match &self.destination {
            HolderSet::Single(key) if key.starts_with('#') => {
                let tag = identifier(&key[1..]);
                quote! { REGISTRY.structures.get_tag(&#tag).ok_or_else(|| crate::loot::OperationError::WorldData(format!("missing exploration-map structure tag {}", #tag)))? }
            }
            HolderSet::Single(key) => {
                let key = format_ident!("STRUCTURE_{}", ident(key));
                quote! { vec![&steel_registry::vanilla_structures::#key] }
            }
            HolderSet::List(keys) => {
                let structures = keys.iter().map(|key| {
                    let key = format_ident!("STRUCTURE_{}", ident(key));
                    quote! { &steel_registry::vanilla_structures::#key }
                });
                quote! { vec![#(#structures),*] }
            }
        };
        let decoration = ident(self.decoration.as_deref().unwrap_or("minecraft:mansion"));
        let Self {
            zoom,
            search_radius,
            skip_existing_chunks,
            ..
        } = self;
        quote! {
            if !item.is_empty() && let Some(origin) = ctx.origin {
                let Some(world) = ctx.world else { return ctx.unsupported("minecraft:exploration_map requires a world"); };
                let world = world.chunk_map.world_gen_context.world();
                let origin = steel_utils::BlockPos::containing(origin.x, origin.y, origin.z);
                let destinations: Vec<steel_registry::structure::StructureRef> = #destinations;
                if #skip_existing_chunks {
                    world.claim_map_structure_at(&destinations, origin).await.map_err(|error| crate::loot::OperationError::WorldData(error.to_string()))?;
                }
                if let Some(target) = world.find_nearest_map_structure(&destinations, origin, #search_radius, #skip_existing_chunks).await.map_err(|error| crate::loot::OperationError::WorldData(error.to_string()))? {
                    crate::behavior::items::MapItem::apply_new_saved_data(&world, item, target.0.x, target.0.z, #zoom, true, true);
                    let Some(data) = crate::behavior::items::MapItem::get_saved_data(&world, item) else { return ctx.unsupported("new exploration map lost its saved data"); };
                    crate::maps::MapItemSavedData::render_biome_preview(&data, &world).await.map_err(|error| crate::loot::OperationError::WorldData(error.to_string()))?;
                    crate::maps::MapItemSavedData::add_target_decoration(item, target, "+".to_owned(), &steel_registry::vanilla_map_decoration_types::#decoration);
                }
            }
        }
    }
}
