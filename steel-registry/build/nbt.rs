//! Shared NBT parsing and code generation for registry build scripts.

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

pub fn parse_lenient_compound(value: &serde_json::Value, context: &str) -> NbtCompound {
    match value {
        serde_json::Value::String(value) => steel_utils::nbt::parse_snbt_compound(value)
            .unwrap_or_else(|error| panic!("failed to parse {context} SNBT: {error}")),
        serde_json::Value::Object(_) => json_value_to_nbt_compound(value, context),
        _ => panic!("{context} must be an object or flattened SNBT string"),
    }
}

fn json_value_to_nbt_compound(value: &serde_json::Value, context: &str) -> NbtCompound {
    let Some(object) = value.as_object() else {
        panic!("{context} must be an object");
    };
    let mut compound = NbtCompound::new();
    for (key, value) in object {
        compound.insert(key.as_str(), json_value_to_nbt_tag(value, context));
    }
    compound
}

fn json_value_to_nbt_tag(value: &serde_json::Value, context: &str) -> NbtTag {
    match value {
        serde_json::Value::Null => panic!("{context} cannot contain null values"),
        serde_json::Value::Bool(value) => NbtTag::Byte(i8::from(*value)),
        serde_json::Value::Number(value) => json_number_to_nbt_tag(value, context),
        serde_json::Value::String(value) => NbtTag::String(value.clone().into()),
        serde_json::Value::Array(values) => NbtTag::List(NbtList::from(
            values
                .iter()
                .map(|value| json_value_to_nbt_tag(value, context))
                .collect::<Vec<_>>(),
        )),
        serde_json::Value::Object(_) => {
            NbtTag::Compound(json_value_to_nbt_compound(value, context))
        }
    }
}

fn json_number_to_nbt_tag(value: &serde_json::Number, context: &str) -> NbtTag {
    if let Some(value) = value.as_i64() {
        return i32::try_from(value)
            .map(NbtTag::Int)
            .unwrap_or_else(|_| NbtTag::Long(value));
    }
    if let Some(value) = value.as_u64() {
        if let Ok(value) = i32::try_from(value) {
            return NbtTag::Int(value);
        }
        return i64::try_from(value).map(NbtTag::Long).unwrap_or_else(|_| {
            panic!("{context} integer value {value} does not fit in an NBT long")
        });
    }
    let Some(value) = value.as_f64() else {
        panic!("invalid {context} number: {value}");
    };
    NbtTag::Double(value)
}

pub fn generate_nbt_compound(compound: &NbtCompound) -> TokenStream {
    let entries = compound.iter().map(|(key, value)| {
        let key = key.to_string();
        let value = generate_nbt_tag(value);
        quote! { (#key.into(), #value) }
    });
    quote! { simdnbt::owned::NbtCompound::from_values(vec![#(#entries),*]) }
}

fn generate_nbt_list(list: &NbtList) -> TokenStream {
    match list {
        NbtList::Empty => quote! { simdnbt::owned::NbtList::Empty },
        NbtList::Byte(values) => quote! { simdnbt::owned::NbtList::Byte(vec![#(#values),*]) },
        NbtList::Short(values) => quote! { simdnbt::owned::NbtList::Short(vec![#(#values),*]) },
        NbtList::Int(values) => quote! { simdnbt::owned::NbtList::Int(vec![#(#values),*]) },
        NbtList::Long(values) => quote! { simdnbt::owned::NbtList::Long(vec![#(#values),*]) },
        NbtList::Float(values) => {
            let values = values.iter().map(|value| Literal::f32_unsuffixed(*value));
            quote! { simdnbt::owned::NbtList::Float(vec![#(#values),*]) }
        }
        NbtList::Double(values) => {
            let values = values.iter().map(|value| Literal::f64_unsuffixed(*value));
            quote! { simdnbt::owned::NbtList::Double(vec![#(#values),*]) }
        }
        NbtList::ByteArray(values) => {
            let values = values.iter().map(|value| quote! { vec![#(#value),*] });
            quote! { simdnbt::owned::NbtList::ByteArray(vec![#(#values),*]) }
        }
        NbtList::String(values) => {
            let values = values
                .iter()
                .map(|value| value.as_str().to_str().into_owned());
            quote! { simdnbt::owned::NbtList::String(vec![#(#values.into()),*]) }
        }
        NbtList::List(values) => {
            let values = values.iter().map(generate_nbt_list);
            quote! { simdnbt::owned::NbtList::List(vec![#(#values),*]) }
        }
        NbtList::Compound(values) => {
            let values = values.iter().map(generate_nbt_compound);
            quote! { simdnbt::owned::NbtList::Compound(vec![#(#values),*]) }
        }
        NbtList::IntArray(values) => {
            let values = values.iter().map(|value| quote! { vec![#(#value),*] });
            quote! { simdnbt::owned::NbtList::IntArray(vec![#(#values),*]) }
        }
        NbtList::LongArray(values) => {
            let values = values.iter().map(|value| quote! { vec![#(#value),*] });
            quote! { simdnbt::owned::NbtList::LongArray(vec![#(#values),*]) }
        }
    }
}

fn generate_nbt_tag(tag: &NbtTag) -> TokenStream {
    match tag {
        NbtTag::Byte(value) => quote! { simdnbt::owned::NbtTag::Byte(#value) },
        NbtTag::Short(value) => quote! { simdnbt::owned::NbtTag::Short(#value) },
        NbtTag::Int(value) => quote! { simdnbt::owned::NbtTag::Int(#value) },
        NbtTag::Long(value) => quote! { simdnbt::owned::NbtTag::Long(#value) },
        NbtTag::Float(value) => {
            let value = Literal::f32_unsuffixed(*value);
            quote! { simdnbt::owned::NbtTag::Float(#value) }
        }
        NbtTag::Double(value) => {
            let value = Literal::f64_unsuffixed(*value);
            quote! { simdnbt::owned::NbtTag::Double(#value) }
        }
        NbtTag::ByteArray(value) => {
            quote! { simdnbt::owned::NbtTag::ByteArray(vec![#(#value),*]) }
        }
        NbtTag::String(value) => {
            let value = value.as_str().to_str().into_owned();
            quote! { simdnbt::owned::NbtTag::String(#value.into()) }
        }
        NbtTag::List(value) => {
            let value = generate_nbt_list(value);
            quote! { simdnbt::owned::NbtTag::List(#value) }
        }
        NbtTag::Compound(value) => {
            let value = generate_nbt_compound(value);
            quote! { simdnbt::owned::NbtTag::Compound(#value) }
        }
        NbtTag::IntArray(value) => {
            quote! { simdnbt::owned::NbtTag::IntArray(vec![#(#value),*]) }
        }
        NbtTag::LongArray(value) => {
            quote! { simdnbt::owned::NbtTag::LongArray(vec![#(#value),*]) }
        }
    }
}
