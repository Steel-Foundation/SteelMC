use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use super::tag_utils;

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/tags/fluid/"
    );

    let tag_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/tags/fluid";
    let all_tags = tag_utils::read_all_tags(tag_dir);
    let sorted_tags = tag_utils::resolve_all_tags(&all_tags);

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::fluid::FluidRegistry;
        use crate::TaggedRegistryExt;
        use steel_utils::Identifier;
    });

    let mut register_stream = TokenStream::new();
    let mut tag_stream = TokenStream::new();
    for (tag_name, fluids) in &sorted_tags {
        let tag_array = Ident::new(
            &format!("{}_TAG_LIST", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let tag_ident = Ident::new(
            &format!("{}_TAG", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );

        let fluid_strs = fluids.iter().map(|s| s.as_str());

        stream.extend(quote! {
            static #tag_array: &[&str] = &[#(#fluid_strs),*];
        });
        let tag_key = tag_name.clone();

        tag_stream.extend(quote! {
            pub const #tag_ident: Identifier = Identifier::vanilla_static(#tag_key);
        });
        register_stream.extend(quote! {
            registry.register_tag(
                #tag_ident,
                #tag_array
            );
        });
    }

    stream.extend(quote! {
        #tag_stream

        pub fn register_fluid_tags(registry: &mut FluidRegistry) {
            #register_stream
        }
    });

    stream
}
