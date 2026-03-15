use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "instrument",
        "instrument",
        "InstrumentRegistry",
        "register_instrument_tags",
    )
}
