use proc_macro2::TokenStream;

pub(crate) fn build() -> TokenStream {
    super::tag_utils::build_simple_tags(
        "dialog",
        "dialog",
        "DialogRegistry",
        "register_dialog_tags",
    )
}
