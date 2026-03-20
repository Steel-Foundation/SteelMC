use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use quote::quote;

use crate::to_block_ident;

pub fn build() -> String {
    println!("cargo:rerun-if-changed=build/weathering.json");

    let oxidizables_json =
        fs::read_to_string("build/weathering.json").expect("Failed to read weathering.json");
    let oxidizables_raw: BTreeMap<String, String> =
        serde_json::from_str(&oxidizables_json).expect("Failed to parse weathering.json");

    // Build the forward map: current -> next
    let oxidizables: Vec<proc_macro2::TokenStream> = oxidizables_raw
        .iter()
        .map(|(current, next)| (to_block_ident(current), to_block_ident(next)))
        .map(|(from, to)| quote! { b if b == vanilla_blocks::#from => Some(vanilla_blocks::#to) , })
        .collect();

    // Build the reverse map: next -> current
    let oxidizables_reverse: Vec<proc_macro2::TokenStream> = oxidizables_raw
        .iter()
        .map(|(current, next)| (to_block_ident(current), to_block_ident(next)))
        .map(|(from, to)| quote! { b if b == vanilla_blocks::#to => Some(vanilla_blocks::#from) , })
        .collect();

    // Derive WeatherState for every block by walking chains.
    // Roots are blocks that appear as keys but never as values (Unaffected stage).
    let all_values: BTreeSet<&String> = oxidizables_raw.values().collect();
    let roots: Vec<&String> = oxidizables_raw
        .keys()
        .filter(|k| !all_values.contains(k))
        .collect();

    let mut weather_state_arms = Vec::new();
    for root in &roots {
        let mut current = *root;
        let stages = ["Unaffected", "Exposed", "Weathered"];

        for stage_name in &stages {
            let block_ident = to_block_ident(current);
            let state_ident = proc_macro2::Ident::new(stage_name, proc_macro2::Span::call_site());
            weather_state_arms.push(
                quote! { b if b == vanilla_blocks::#block_ident => Some(WeatherState::#state_ident) , },
            );

            let Some(next) = oxidizables_raw.get(current) else {
                break;
            };
            current = next;
        }

        // The final block in the chain is Oxidized
        let block_ident = to_block_ident(current);
        weather_state_arms.push(
            quote! { b if b == vanilla_blocks::#block_ident => Some(WeatherState::Oxidized) , },
        );
    }

    let output: proc_macro2::TokenStream = quote! {
        use steel_registry::{blocks::BlockRef, vanilla_blocks};
        use crate::behavior::blocks::WeatherState;

        #[must_use]
        #[inline]
        pub fn next_copper_stage(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#oxidizables)*
                _ => None
            }
        }

        #[must_use]
        #[inline]
        pub fn previous_copper_stage(block: BlockRef) -> Option<BlockRef> {
            match block {
                #(#oxidizables_reverse)*
                _ => None
            }
        }

        /// Returns the weathering state of a copper block, or `None` if it's not a weathering block.
        #[must_use]
        #[inline]
        pub fn get_weather_state(block: BlockRef) -> Option<WeatherState> {
            match block {
                #(#weather_state_arms)*
                _ => None
            }
        }
    };

    output.to_string()
}
