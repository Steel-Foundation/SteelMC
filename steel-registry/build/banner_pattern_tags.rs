use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "banner_pattern",
        "banner_pattern",
        "BannerPatternRegistry",
        "register_banner_pattern_tags",
    )
}
