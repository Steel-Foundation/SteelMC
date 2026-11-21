use std::fs;

use heck::ToUpperCamelCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

const ASSET: &str = "build_assets/entity_variant_enums.json";

#[derive(Deserialize)]
struct VariantEnums {
    horse_variant: Vec<Entry>,
    horse_markings: Vec<Entry>,
    llama_variant: Vec<Entry>,
    parrot_variant: Vec<Entry>,
    panda_gene: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    id: i32,
    serialized_name: Option<String>,
}

enum OutOfBounds {
    Zero,
    Clamp,
    Wrap,
}

pub(super) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed={ASSET}");
    let input = fs::read_to_string(ASSET).unwrap_or_else(|err| panic!("Read {ASSET}: {err}"));
    let variants: VariantEnums =
        serde_json::from_str(&input).unwrap_or_else(|err| panic!("Parse {ASSET}: {err}"));
    [
        ("HorseVariant", variants.horse_variant, OutOfBounds::Wrap),
        ("HorseMarkings", variants.horse_markings, OutOfBounds::Wrap),
        ("LlamaVariant", variants.llama_variant, OutOfBounds::Clamp),
        ("ParrotVariant", variants.parrot_variant, OutOfBounds::Clamp),
        ("PandaGene", variants.panda_gene, OutOfBounds::Zero),
    ]
    .into_iter()
    .map(|(name, entries, out_of_bounds)| generate(name, &entries, out_of_bounds))
    .collect()
}

fn generate(name: &str, entries: &[Entry], out_of_bounds: OutOfBounds) -> TokenStream {
    assert!(!entries.is_empty(), "{name} has no entries");
    let len = entries.len();
    let last_id = i32::try_from(len - 1).unwrap();
    let enum_name = format_ident!("{name}");
    let names = entries
        .iter()
        .map(|entry| format_ident!("{}", entry.name.to_upper_camel_case()))
        .collect::<Vec<_>>();
    let ids = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();

    let mut by_id = names.iter().zip(&ids).collect::<Vec<_>>();
    by_id.sort_unstable_by_key(|&(_, id)| *id);
    assert!(
        by_id.iter().map(|&(_, id)| *id).eq(0..=last_id),
        "{name} needs unique, contiguous IDs starting at zero"
    );

    let by_id = by_id
        .into_iter()
        .map(|(variant, _)| variant)
        .collect::<Vec<_>>();

    let normalize_id = match out_of_bounds {
        OutOfBounds::Zero => quote! {
            match id { 0..=#last_id => id, _ => 0 }
        },
        OutOfBounds::Clamp => quote! {
            match id {
                i32::MIN..=-1 => 0,
                #last_id..=i32::MAX => #last_id,
                _ => id,
            }
        },
        OutOfBounds::Wrap => {
            let len = last_id + 1;
            quote! { id.rem_euclid(#len) }
        }
    };

    let declarations = names.iter().zip(&ids).map(|(variant, id)| {
        let default = (*id == 0).then(|| quote! { #[default] });
        quote! { #default #variant }
    });

    let serialized_names = entries
        .iter()
        .map(|entry| entry.serialized_name.as_deref())
        .collect::<Option<Vec<_>>>();

    let name_methods = if let Some(serialized_names) = serialized_names {
        quote! {
            #[must_use]
            pub const fn serialized_name(self) -> &'static str {
                match self { #(Self::#names => #serialized_names,)* }
            }

            #[must_use]
            pub const fn from_serialized_name(name: &str) -> Option<Self> {
                match name {
                    #(#serialized_names => Some(Self::#names),)*
                    _ => None,
                }
            }
        }
    } else {
        assert!(
            entries.iter().all(|entry| entry.serialized_name.is_none()),
            "{name} has entries with missing serialized names"
        );
        TokenStream::new()
    };

    quote! {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum #enum_name {
            #(#declarations,)*
        }

        impl #enum_name {
            /// Variants in mc declaration order
            pub const VALUES: [Self; #len] = [#(Self::#names),*];

            #[must_use]
            pub const fn id(self) -> i32 {
                match self { #(Self::#names => #ids,)* }
            }

            #[must_use]
            pub const fn by_id(id: i32) -> Self {
                const BY_ID: [#enum_name; #len] = [#(#enum_name::#by_id),*];
                let index = #normalize_id;
                BY_ID[index as usize]
            }

            #name_methods
        }
    }
}
