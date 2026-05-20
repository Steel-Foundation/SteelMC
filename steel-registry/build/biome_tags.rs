use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "worldgen/biome",
        "biome",
        "BiomeRegistry",
        "register_biome_tags",
    )
}
