use crate::generator_functions::{generate_identifier, read_json_asset};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct BannerPatternJson {
    asset_id: Identifier,
    translation_key: String,
}

pub(crate) fn build() -> TokenStream {
    // BannerPatterns.bootstrap defines registry insertion order in Vanilla.
    const VANILLA_ORDER: &[&str] = &[
        "base",
        "square_bottom_left",
        "square_bottom_right",
        "square_top_left",
        "square_top_right",
        "stripe_bottom",
        "stripe_top",
        "stripe_left",
        "stripe_right",
        "stripe_center",
        "stripe_middle",
        "stripe_downright",
        "stripe_downleft",
        "small_stripes",
        "cross",
        "straight_cross",
        "triangle_bottom",
        "triangle_top",
        "triangles_bottom",
        "triangles_top",
        "diagonal_left",
        "diagonal_up_right",
        "diagonal_up_left",
        "diagonal_right",
        "circle",
        "rhombus",
        "half_vertical",
        "half_horizontal",
        "half_vertical_right",
        "half_horizontal_bottom",
        "border",
        "gradient",
        "gradient_up",
        "bricks",
        "curly_border",
        "globe",
        "creeper",
        "skull",
        "flower",
        "mojang",
        "piglin",
        "flow",
        "guster",
    ];

    let patterns = VANILLA_ORDER.iter().map(|name| {
        let path = format!(
            "../steel-utils/build_assets/builtin_datapacks/minecraft/banner_pattern/{name}.json"
        );
        (*name, read_json_asset::<BannerPatternJson>(&path))
    });

    let mut definitions = TokenStream::new();
    let mut registrations = TokenStream::new();
    for (name, pattern) in patterns {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        let asset_id = generate_identifier(&pattern.asset_id);
        let translation_key = pattern.translation_key;

        definitions.extend(quote! {
            pub static #ident: BannerPattern = BannerPattern::new(
                #key,
                BannerPatternValue::new(#asset_id, Cow::Borrowed(#translation_key)),
            );
        });
        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::banner_pattern::{BannerPattern, BannerPatternRegistry, BannerPatternValue};
        use steel_utils::Identifier;
        use std::borrow::Cow;

        #definitions

        pub fn register_banner_patterns(registry: &mut BannerPatternRegistry) {
            #registrations
        }
    }
}
