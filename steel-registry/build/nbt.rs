//! Shared NBT code generation for registry build scripts.

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

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
