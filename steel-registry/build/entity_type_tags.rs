use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "entity_type",
        "entity_types",
        "EntityTypeRegistry",
        "register_entity_type_tags",
    )
}
