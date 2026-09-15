use crate::advancements::criterion::{CriterionJson, parse_criteria};
use crate::advancements::display::{AdvancementDisplayJson, parse_display};
use crate::generator_functions::{generate_identifier, generate_option, generate_vec};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use steel_utils::Identifier;

mod criterion;
mod display;

#[derive(Deserialize)]
struct AdvancementJson {
    parent: Option<Identifier>,
    criteria: BTreeMap<String, CriterionJson>,
    display: Option<AdvancementDisplayJson>,
    #[serde(default)]
    send_telemetry_event: bool,
    requirements: Vec<Vec<String>>,
    #[serde(default)]
    rewards: AdvancementRewardsJson,
}

#[derive(Deserialize, Default)]
struct AdvancementRewardsJson {
    #[serde(default)]
    experience: i32,
    #[serde(default)]
    loots: Vec<Identifier>,
    #[serde(default)]
    recipes: Vec<Identifier>,
    function: Option<Identifier>,
}

impl ToTokens for AdvancementRewardsJson {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let experience = self.experience;
        let recipes = generate_vec(&self.recipes, |recipe| {
            let name = Ident::new(&recipe.path.to_shouty_snake_case(), Span::call_site());
            quote! {
                UntypedRecipeRef { recipe: &*vanilla_recipes::#name }
            }
        });
        let loots = generate_vec(&self.loots, |loot| {
            let name = Ident::new(&loot.path.to_shouty_snake_case(), Span::call_site());
            quote! {
                &*vanilla_lo::#name
            }
        });
        let function = generate_option(&self.function, generate_identifier);
        tokens.extend(quote! {
            AdvancementRewards {
                experience: #experience,
                recipes: #recipes,
                loots: #loots,
                function: #function,
            }
        });
    }
}

struct AdvancementData {
    /// Full key path like "`advanture/root`"
    key: String,
    /// Rust identifier like "`BLOCKS_ACACIA_BUTTON`"
    const_ident: Ident,
    /// The loot type as a `TokenStream`
    advancement: AdvancementJson,
}

impl ToTokens for AdvancementData {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let const_ident = &self.const_ident;
        let name = &self.key;
        let parent = generate_option(&self.advancement.parent, generate_identifier);
        let criteria = parse_criteria(&self.advancement.criteria);
        let display = generate_option(&self.advancement.display, parse_display);
        let send_telemetry_event = self.advancement.send_telemetry_event;
        let requirements = generate_vec(&self.advancement.requirements, |v| {
            generate_vec(v, |v2| quote! {#v2})
        });
        let rewards = &self.advancement.rewards;
        tokens.extend(quote! {
             static #const_ident: LazyLock<Advancement> = LazyLock::new(|| Advancement {
                key: Identifier::vanilla_static(#name),
                parent: #parent,
                criteria: #criteria,
                display: #display,
                send_telemetry_event: #send_telemetry_event,
                requirements: #requirements,
                rewards: #rewards,
            });
        });
    }
}

pub(crate) fn build() -> TokenStream {
    let advancement_dir =
        Path::new("../steel-utils/build_assets/builtin_datapacks/minecraft/advancement");
    println!("cargo:rerun-if-changed={}", advancement_dir.display());
    let mut advancements = read_advancements(advancement_dir);
    advancements.sort_by(|a, b| a.key.cmp(&b.key));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::advancement::display::{AdvancementType,DisplayInfo};
        use crate::advancement::{Advancement,AdvancementRewards,Criterion};
        use crate::advancement::registry::AdvancementRegistry;
        use crate::vanilla_items;
        use crate::vanilla_recipes;
        use crate::ItemStackTemplate;
        use crate::recipe::UntypedRecipeRef;
        use text_components::translation::TranslatedMessage;
        use std::collections::BTreeMap;
        use std::borrow::Cow;
        use std::sync::LazyLock;
        use text_components::TextComponent;
        use steel_utils::Identifier;
    });

    let mut register_stream = TokenStream::new();

    for advancement_data in &advancements {
        advancement_data.to_tokens(&mut stream);
        let const_ident = &advancement_data.const_ident;
        register_stream.extend(quote! {
            registry.register(&#const_ident);
        });
    }
    stream.extend(quote! {
        pub fn register_advancements(registry: &mut AdvancementRegistry) {
            #register_stream
        }
    });
    stream
}

fn read_advancements(dir: &Path) -> Vec<AdvancementData> {
    let mut advancements = Vec::new();
    read_advancements_from(dir, dir, &mut advancements);
    advancements
}

fn read_advancements_from(root: &Path, dir: &Path, advancements: &mut Vec<AdvancementData>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|error| {
        panic!(
            "Cannot read advancement directory {}: {error}",
            dir.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "Cannot read an entry in advancement directory {}: {error}",
                dir.display()
            )
        });
        let path = entry.path();
        if path.is_dir() {
            read_advancements_from(root, &path, advancements);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let key = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .with_extension("")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("Cannot read advancement {}: {error}", path.display()));
        let const_name = key.replace('/', "_").to_shouty_snake_case();
        let const_ident = Ident::new(&const_name, Span::call_site());
        let advancement: AdvancementJson = serde_json::from_str(&source)
            .unwrap_or_else(|e| panic!("Failed to parse advancement {const_name}: {e}"));

        advancements.push(AdvancementData {
            key,
            const_ident,
            advancement,
        });
    }
}
