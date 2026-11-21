use serde::Deserialize;

use super::super::{
    Compiler, TokenStream,
    model::{CompileModifier, Condition, Holder},
    quote,
};

/// Compiles the static names used by map loot. Entity-resolved components and
/// additional text codecs require their own typed compiler inputs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetName {
    name: Option<Name>,
    #[serde(default)]
    target: Target,
    pub condition: Option<Holder<Condition>>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Target {
    #[default]
    CustomName,
    ItemName,
}

impl CompileModifier for SetName {
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        let Some(name) = &self.name else {
            return quote! {};
        };
        let name = name.compile();
        let target = match self.target {
            Target::CustomName => {
                quote! { steel_registry::data_components::vanilla_components::CUSTOM_NAME }
            }
            Target::ItemName => {
                quote! { steel_registry::data_components::vanilla_components::ITEM_NAME }
            }
        };
        quote! { item.set(#target, #name); }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Name {
    Literal(String),
    Plain(PlainName),
    Translated(TranslatedName),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlainName {
    text: String,
    #[serde(default)]
    extra: Vec<Name>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslatedName {
    translate: String,
    fallback: Option<String>,
    with: Option<Vec<Name>>,
    #[serde(default)]
    extra: Vec<Name>,
}

impl Name {
    fn compile(&self) -> TokenStream {
        let (content, extra) = match self {
            Self::Literal(text) => {
                return quote! { text_components::TextComponent::const_plain(#text) };
            }
            Self::Plain(PlainName { text, extra }) => (
                quote! { text_components::content::Content::Text { text: #text.into() } },
                extra,
            ),
            Self::Translated(name) => {
                let translate = &name.translate;
                let fallback = name.fallback.as_ref().map_or_else(
                    || quote! { None },
                    |fallback| quote! { Some(#fallback.into()) },
                );
                let args = name.with.as_ref().map_or_else(
                    || quote! { None },
                    |args| {
                        let args = args.iter().map(Self::compile);
                        quote! { Some(vec![#(#args),*].into_boxed_slice()) }
                    },
                );
                (
                    quote! { text_components::content::Content::Translate(
                        text_components::translation::TranslatedMessage {
                            key: #translate.into(), fallback: #fallback, args: #args,
                        }
                    ) },
                    &name.extra,
                )
            }
        };
        let children = extra.iter().map(Self::compile);
        quote! { text_components::TextComponent {
            content: #content, children: vec![#(#children),*],
            ..text_components::TextComponent::new()
        } }
    }
}
