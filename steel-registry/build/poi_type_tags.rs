use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "point_of_interest_type",
        "poi",
        "PoiTypeRegistry",
        "register_poi_type_tags",
    )
}
