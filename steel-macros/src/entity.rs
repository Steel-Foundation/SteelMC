//! `#[derive(Entity)]`: the `Entity` impl and component access for a struct of components.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Ident, Path, Type, parse2, spanned::Spanned};

#[derive(Default)]
struct StructOptions {
    hooks: Option<Expr>,
    entity_type: Option<Expr>,
    save: Option<Path>,
    load: Option<Path>,
    /// Vanilla saves nothing for the type beyond the base entity data, even
    /// for its components (an empty `addAdditionalSaveData`).
    no_save_data: bool,
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

    Ok(quote! {
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

            #synced_data
            #erased_component
            #persistence
        }

        #(#has_impls)*
    })
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
            } else {
                return Err(
                    meta.error("expected `hooks`, `entity_type`, `save`, `load` or `no_save_data`")
                );
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
