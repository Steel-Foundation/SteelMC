//! `#[derive(Entity)]`: the `Entity` impl and component access for a struct of components.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitStr, Path, Type, parse2, spanned::Spanned};

#[derive(Default)]
struct StructOptions {
    hooks: Option<Expr>,
    entity_type: Option<Expr>,
    save: Option<Path>,
    load: Option<Path>,
    /// Vanilla saves nothing for the type beyond the base entity data, even
    /// for its components (an empty `addAdditionalSaveData`).
    no_save_data: bool,
    /// The owner-scoped downcast key; generates the `DowncastType` impl.
    key: Option<LitStr>,
    /// Generates `new` and `from_saved` around the type's `from_base`.
    constructors: bool,
    /// The mob definition builder; generates a cached `definition()`.
    definition: Option<Expr>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldRole {
    Base,
    EntityType,
    Component,
    Synced,
}

struct RoleField {
    role: FieldRole,
    ident: Ident,
    ty: Type,
}

pub fn derive(input: TokenStream) -> TokenStream {
    match expand(input) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}

fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = parse2(input)?;
    let options = struct_options(&input)?;
    let fields = role_fields(&input)?;

    let single = |role: FieldRole, name: &str| -> syn::Result<Option<&RoleField>> {
        let mut matching = fields.iter().filter(|field| field.role == role);
        let first = matching.next();
        if let Some(extra) = matching.next() {
            return Err(syn::Error::new(
                extra.ident.span(),
                format!("only one field may be `#[entity({name})]`"),
            ));
        }
        Ok(first)
    };
    let base = single(FieldRole::Base, "base")?
        .ok_or_else(|| syn::Error::new(input.ident.span(), "missing `#[entity(base)]` field"))?;
    let entity_type = entity_type_tokens(
        &input,
        &options,
        single(FieldRole::EntityType, "entity_type")?,
    )?;
    let synced = single(FieldRole::Synced, "synced")?;
    let components: Vec<&RoleField> = fields
        .iter()
        .filter(|field| field.role == FieldRole::Component)
        .collect();

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let base_ident = &base.ident;

    let hooks = options.hooks.as_ref().map_or_else(
        || quote! { &crate::entity::EntityDefaults },
        |hooks| quote! { &#hooks },
    );

    let synced_data = synced.map(|field| {
        let ident = &field.ident;
        quote! {
            fn synced_data(&self) -> ::core::option::Option<&dyn crate::entity::EntitySyncedData> {
                ::core::option::Option::Some(&self.#ident)
            }
        }
    });

    let erased_component = erased_component_tokens(&components);
    let persistence = persistence_tokens(&options, &components);

    let has_impls = components.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.ty;
        quote! {
            impl #impl_generics crate::entity::component::Has<#ty> for #name #ty_generics #where_clause {
                fn component(&self) -> &#ty {
                    &self.#ident
                }
            }
        }
    });

    let extra = extra_tokens(&input, &options)?;

    Ok(quote! {
        #extra

        impl #impl_generics crate::entity::Entity for #name #ty_generics #where_clause {
            fn base(&self) -> &crate::entity::EntityBase {
                &self.#base_ident
            }

            fn entity_type(&self) -> ::steel_registry::entity_type::EntityTypeRef {
                #entity_type
            }

            fn entity_hooks(&self) -> &'static dyn crate::entity::EntityHooks {
                #hooks
            }

            fn entity_definition(
                &self,
            ) -> &'static dyn crate::entity::component::definition::DefinitionEntries {
                Self::definition()
            }

            #synced_data
            #erased_component
            #persistence
        }

        #(#has_impls)*
    })
}

/// The downcast impl, constructors and cached definition the struct options ask for.
fn extra_tokens(input: &DeriveInput, options: &StructOptions) -> syn::Result<TokenStream> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            name.span(),
            "`#[derive(Entity)]` needs a non-generic entity type",
        ));
    }

    let downcast = options.key.as_ref().map(|key| {
        quote! {
            // SAFETY: The key is owned by Steel and unique to this entity type.
            unsafe impl ::steel_utils::DowncastType for #name {
                const TYPE_KEY: ::steel_utils::DowncastTypeKey =
                    ::steel_utils::DowncastTypeKey::new(#key);
            }
        }
    });

    let constructors = options.constructors.then(|| {
        quote! {
            /// Creates an entity of this type at `position`.
            #[must_use]
            pub fn new(
                entity_type: ::steel_registry::entity_type::EntityTypeRef,
                id: i32,
                position: ::glam::DVec3,
                world: ::std::sync::Weak<crate::world::World>,
            ) -> ::std::sync::Arc<Self> {
                crate::entity::WeakSelf::build(|this| {
                    Self::from_base(
                        crate::entity::EntityBase::new(
                            id,
                            position,
                            entity_type.dimensions,
                            world,
                            this,
                        ),
                        entity_type,
                    )
                })
            }

            /// Reconstructs an entity of this type from saved base data.
            #[must_use]
            pub fn from_saved(
                entity_type: ::steel_registry::entity_type::EntityTypeRef,
                load: crate::entity::EntityBaseLoad,
            ) -> ::std::sync::Arc<Self> {
                crate::entity::WeakSelf::build(|this| {
                    Self::from_base(
                        crate::entity::EntityBase::from_load(
                            load,
                            entity_type.dimensions,
                            this,
                        ),
                        entity_type,
                    )
                })
            }
        }
    });

    let Some(define) = options.definition.as_ref() else {
        return Err(syn::Error::new(
            name.span(),
            "missing `#[entity(definition = ..)]`: name the type's `define` or the \
             layer definition it uses unchanged, such as `Definition::new`",
        ));
    };
    // A nested `static` cannot name `Self`.
    let define = replace_self(quote! { #define }, name);
    let definition = quote! {
        /// This type's pipelines, built once.
        fn definition() -> &'static crate::entity::component::definition::Definition<Self> {
            static DEFINITION: ::std::sync::LazyLock<
                crate::entity::component::definition::Definition<#name>,
            > = ::std::sync::LazyLock::new(#define);
            &DEFINITION
        }
    };

    let inherent = quote! {
        impl #name {
            #constructors
            #definition
        }
    };

    Ok(quote! {
        #downcast
        #inherent
    })
}

/// Replaces every `Self` in `tokens` with `name`.
fn replace_self(tokens: TokenStream, name: &Ident) -> TokenStream {
    use proc_macro2::{Group, TokenTree};
    tokens
        .into_iter()
        .map(|token| match token {
            TokenTree::Ident(ident) if ident == "Self" => TokenTree::Ident(name.clone()),
            TokenTree::Group(group) => {
                let mut replaced =
                    Group::new(group.delimiter(), replace_self(group.stream(), name));
                replaced.set_span(group.span());
                TokenTree::Group(replaced)
            }
            other => other,
        })
        .collect()
}

fn entity_type_tokens(
    input: &DeriveInput,
    options: &StructOptions,
    field: Option<&RoleField>,
) -> syn::Result<TokenStream> {
    match (field, &options.entity_type) {
        (Some(field), None) => {
            let ident = &field.ident;
            Ok(quote! { self.#ident })
        }
        (None, Some(entity_type)) => Ok(quote! { #entity_type }),
        (Some(field), Some(_)) => Err(syn::Error::new(
            field.ident.span(),
            "the entity type is both a field and a struct option",
        )),
        (None, None) => Err(syn::Error::new(
            input.ident.span(),
            "missing `#[entity(entity_type)]` field or `#[entity(entity_type = ..)]` option",
        )),
    }
}

fn erased_component_tokens(components: &[&RoleField]) -> Option<TokenStream> {
    if components.is_empty() {
        return None;
    }
    let idents = components.iter().map(|field| &field.ident);
    let types = components.iter().map(|field| &field.ty);
    Some(quote! {
        fn erased_component(
            &self,
            key: ::steel_utils::DowncastTypeKey,
        ) -> ::core::option::Option<&dyn ::steel_utils::ErasedType> {
            #(
                if key == <#types as ::steel_utils::DowncastType>::TYPE_KEY {
                    return ::core::option::Option::Some(&self.#idents);
                }
            )*
            ::core::option::Option::None
        }
    })
}

fn persistence_tokens(options: &StructOptions, components: &[&RoleField]) -> Option<TokenStream> {
    if options.no_save_data
        || components.is_empty() && options.save.is_none() && options.load.is_none()
    {
        return None;
    }
    let idents: Vec<&Ident> = components.iter().map(|field| &field.ident).collect();
    let save_extra = options
        .save
        .as_ref()
        .map(|save| quote! { #save(self, nbt); });
    let load_extra = options
        .load
        .as_ref()
        .map(|load| quote! { #load(self, nbt); });
    Some(quote! {
        fn save_additional(&self, nbt: &mut ::simdnbt::owned::NbtCompound) {
            #(
                crate::entity::component::Component::save(&self.#idents, self, nbt);
            )*
            #save_extra
        }

        fn load_additional(&self, nbt: ::simdnbt::borrow::NbtCompound<'_, '_>) {
            #(
                crate::entity::component::Component::load(&self.#idents, self, nbt);
            )*
            #load_extra
        }
    })
}

fn struct_options(input: &DeriveInput) -> syn::Result<StructOptions> {
    let mut options = StructOptions::default();
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("entity"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("hooks") {
                options.hooks = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("entity_type") {
                options.entity_type = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("save") {
                options.save = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("load") {
                options.load = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("no_save_data") {
                options.no_save_data = true;
            } else if meta.path.is_ident("key") {
                options.key = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("constructors") {
                options.constructors = true;
            } else if meta.path.is_ident("definition") {
                options.definition = Some(meta.value()?.parse()?);
            } else {
                return Err(meta.error(
                    "expected `hooks`, `entity_type`, `save`, `load`, `no_save_data`, `key`, \
                     `constructors` or `definition`",
                ));
            }
            Ok(())
        })?;
    }
    if options.no_save_data && (options.save.is_some() || options.load.is_some()) {
        return Err(syn::Error::new(
            input.ident.span(),
            "`no_save_data` cannot be combined with `save` or `load`",
        ));
    }
    Ok(options)
}

fn role_fields(input: &DeriveInput) -> syn::Result<Vec<RoleField>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "`Entity` can only be derived for structs",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            data.fields.span(),
            "`Entity` needs named fields",
        ));
    };

    let mut fields = Vec::new();
    for field in &named.named {
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("entity"))
        {
            let mut role = None;
            attr.parse_nested_meta(|meta| {
                let parsed = if meta.path.is_ident("base") {
                    FieldRole::Base
                } else if meta.path.is_ident("entity_type") {
                    FieldRole::EntityType
                } else if meta.path.is_ident("component") {
                    FieldRole::Component
                } else if meta.path.is_ident("synced") {
                    FieldRole::Synced
                } else {
                    return Err(
                        meta.error("expected `base`, `entity_type`, `component` or `synced`")
                    );
                };
                if role.replace(parsed).is_some() {
                    return Err(meta.error("a field has one entity role"));
                }
                Ok(())
            })?;
            if let (Some(role), Some(ident)) = (role, field.ident.clone()) {
                fields.push(RoleField {
                    role,
                    ident,
                    ty: field.ty.clone(),
                });
            }
        }
    }
    Ok(fields)
}
