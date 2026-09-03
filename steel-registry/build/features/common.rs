use std::path::{Path, PathBuf};

use super::{
    BlockHolderSet, BlockStateData, Direction, FluidStateData, Ident, Identifier, IdentifierList,
    Rotation, Span, ToShoutySnakeCase, TokenStream, VerticalAnchor, fs, quote,
};

/// Recursively collects every `.json` file under `dir`, sorted for
/// deterministic codegen output.
pub(super) fn sorted_json_files(dir: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("{dir:?} missing: {err}", dir = dir));
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    files.sort();
    files
}

/// Derives the vanilla resource path (e.g. `coral/tube_block`) for a file
/// found under `dir` by `sorted_json_files`.
pub(super) fn resource_name(dir: &str, path: &Path) -> String {
    path.with_extension("")
        .strip_prefix(dir)
        .unwrap_or_else(|_| panic!("{path:?} is not under {dir}"))
        .to_str()
        .unwrap_or_else(|| panic!("invalid feature file name: {path:?}"))
        .replace('\\', "/")
}

pub(super) fn generate_identifier(identifier: &Identifier) -> TokenStream {
    let namespace = identifier.namespace.as_ref();
    let path = identifier.path.as_ref();
    if namespace == Identifier::VANILLA_NAMESPACE {
        quote! { Identifier::vanilla_static(#path) }
    } else {
        quote! { Identifier::new_static(#namespace, #path) }
    }
}

pub(super) fn vanilla_registry_ident(identifier: &Identifier, kind: &str) -> Ident {
    assert!(
        identifier.namespace == Identifier::VANILLA_NAMESPACE,
        "vanilla feature references non-vanilla {kind} {identifier}"
    );

    Ident::new(&identifier.path.to_shouty_snake_case(), Span::call_site())
}

pub(super) fn generate_block_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "block");
    quote! { &vanilla_blocks::#ident }
}

pub(super) fn generate_fluid_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "fluid");
    quote! { &vanilla_fluids::#ident }
}

pub(super) fn generate_configured_feature_entry_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "configured feature");
    quote! { &crate::vanilla_configured_features::#ident }
}

pub(super) fn generate_placed_feature_entry_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "placed feature");
    quote! { &crate::vanilla_placed_features::#ident }
}

pub(super) fn generate_block_state_provider_entry_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "block state provider");
    quote! { &crate::vanilla_block_state_providers::#ident }
}

pub(super) fn generate_vec<T>(values: &[T], f: impl Fn(&T) -> TokenStream) -> TokenStream {
    let values = values.iter().map(f);
    quote! { vec![#(#values),*] }
}

pub(super) fn generate_option<T>(value: &Option<T>, f: impl Fn(&T) -> TokenStream) -> TokenStream {
    if let Some(value) = value {
        let value = f(value);
        quote! { Some(#value) }
    } else {
        quote! { None }
    }
}

pub(super) fn generate_box<T>(value: &T, f: impl Fn(&T) -> TokenStream) -> TokenStream {
    let value = f(value);
    quote! { Box::new(#value) }
}

pub(super) fn generate_offset(offset: &[i32; 3]) -> TokenStream {
    let [x, y, z] = *offset;
    quote! { IVec3::new(#x, #y, #z) }
}

pub(super) fn generate_block_ref_list(list: &IdentifierList) -> TokenStream {
    let values = generate_vec(&list.0, generate_block_ref);
    quote! { BlockRefList(#values) }
}

pub(super) fn generate_block_holder_set(set: &BlockHolderSet) -> TokenStream {
    match set {
        BlockHolderSet::Tag(tag) => {
            let tag = generate_identifier(tag);
            quote! { BlockHolderSet::Tag(#tag) }
        }
        BlockHolderSet::Entries(entries) => {
            let entries = generate_vec(entries, generate_block_ref);
            quote! { BlockHolderSet::Entries(#entries) }
        }
    }
}

pub(super) fn generate_fluid_ref_list(list: &IdentifierList) -> TokenStream {
    let values = generate_vec(&list.0, generate_fluid_ref);
    quote! { FluidRefList(#values) }
}

pub(super) fn generate_block_state_data(data: &BlockStateData) -> TokenStream {
    let block = generate_block_ref(&data.name);
    let properties = if data.properties.is_empty() {
        quote! { &[] }
    } else {
        let entries = data.properties.iter().map(|(key, value)| {
            let key = key.as_str();
            let value = value.as_str();
            quote! { (#key, #value) }
        });
        quote! { &[#(#entries),*] }
    };

    quote! {
        BlockStateData {
            block: #block,
            properties: #properties,
        }
    }
}

pub(super) fn generate_fluid_state_data(data: &FluidStateData) -> TokenStream {
    let fluid = generate_fluid_ref(&data.name);
    let properties = if data.properties.is_empty() {
        quote! { &[] }
    } else {
        let entries = data.properties.iter().map(|(key, value)| {
            let key = key.as_str();
            let value = value.as_str();
            quote! { (#key, #value) }
        });
        quote! { &[#(#entries),*] }
    };

    quote! {
        FluidStateData {
            fluid: #fluid,
            properties: #properties,
        }
    }
}

pub(super) fn generate_direction(direction: Direction) -> TokenStream {
    match direction {
        Direction::Down => quote! { Direction::Down },
        Direction::Up => quote! { Direction::Up },
        Direction::North => quote! { Direction::North },
        Direction::South => quote! { Direction::South },
        Direction::West => quote! { Direction::West },
        Direction::East => quote! { Direction::East },
    }
}

pub(super) fn generate_rotation(rotation: Rotation) -> TokenStream {
    match rotation {
        Rotation::None => quote! { Rotation::None },
        Rotation::Clockwise90 => quote! { Rotation::Clockwise90 },
        Rotation::Clockwise180 => quote! { Rotation::Clockwise180 },
        Rotation::CounterClockwise90 => quote! { Rotation::CounterClockwise90 },
    }
}

pub(super) fn generate_vertical_anchor(anchor: VerticalAnchor) -> TokenStream {
    match anchor {
        VerticalAnchor::Absolute(value) => quote! { VerticalAnchor::Absolute(#value) },
        VerticalAnchor::AboveBottom(value) => quote! { VerticalAnchor::AboveBottom(#value) },
        VerticalAnchor::BelowTop(value) => quote! { VerticalAnchor::BelowTop(#value) },
        VerticalAnchor::RelativeToSeaLevel(value) => {
            quote! { VerticalAnchor::RelativeToSeaLevel(#value) }
        }
    }
}
