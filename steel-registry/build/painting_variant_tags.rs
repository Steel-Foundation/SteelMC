use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "painting_variant",
        "painting_variant",
        "PaintingVariantRegistry",
        "register_painting_variant_tags",
    )
}
