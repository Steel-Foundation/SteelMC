use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "damage_type",
        "damage_type",
        "DamageTypeRegistry",
        "register_damage_type_tags",
    )
}
