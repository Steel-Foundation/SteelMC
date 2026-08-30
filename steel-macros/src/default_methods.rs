use proc_macro2::{Group, Ident as TokenIdent, Span, TokenStream, TokenTree};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{
    Attribute, FnArg, GenericParam, Ident, ItemTrait, Lifetime, Meta, Pat, ReceiverKind,
    ReturnType, TraitItem, TraitItemFn, Type, TypeReference, parse_quote,
};

pub fn expand(original: &mut ItemTrait) -> syn::Result<TokenStream> {
    let trait_name = &original.ident;
    let defaults_name = format_ident!("{}Defaults", trait_name);
    let visibility = &original.vis;
    let conditional_attributes: Vec<_> = original
        .attrs
        .iter()
        .filter(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        })
        .collect();
    let trait_generics = &original.generics;
    let (_, trait_arguments, trait_where_clause) = trait_generics.split_for_impl();

    let mut default_methods = Vec::new();
    for item in &mut original.items {
        let TraitItem::Fn(method) = item else {
            continue;
        };
        if !take_default_method_marker(&mut method.attrs)? {
            continue;
        }
        if method.default.is_none() {
            return Err(syn::Error::new_spanned(
                method,
                "`default_method` requires a provided trait method",
            ));
        }

        default_methods.push(default_method(method)?);
        method.attrs.push(parse_quote!(
            #[allow(
                unfulfilled_lint_expectations,
                reason = "the lint expectation applies to the default body generated in the companion trait"
            )]
        ));
        method.default = Some(forwarding_body(method, &defaults_name, &trait_arguments)?);
        method.semi_token = None;
    }

    let implementor = unused_implementor_name(trait_generics);
    let mut blanket_generics = trait_generics.clone();
    for parameter in &mut blanket_generics.params {
        match parameter {
            GenericParam::Type(parameter) => parameter.default = None,
            GenericParam::Const(parameter) => parameter.default = None,
            GenericParam::Lifetime(_) => {}
        }
    }
    blanket_generics.params.push(parse_quote!(#implementor));
    blanket_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#implementor: #trait_name #trait_arguments + ?Sized));
    let (blanket_impl_generics, _, blanket_where_clause) = blanket_generics.split_for_impl();
    let defaults_doc =
        format!("Explicit access to the default method implementations of [`{trait_name}`].");

    Ok(quote! {
        #original

        #(#conditional_attributes)*
        #[doc = #defaults_doc]
        #visibility trait #defaults_name #trait_generics:
            #trait_name #trait_arguments
            #trait_where_clause
        {
            #(#default_methods)*
        }

        #(#conditional_attributes)*
        impl #blanket_impl_generics #defaults_name #trait_arguments for #implementor
            #blanket_where_clause
        {}
    })
}

fn take_default_method_marker(attributes: &mut Vec<Attribute>) -> syn::Result<bool> {
    let mut marker = None;
    let mut error = None;
    attributes.retain(|attribute| {
        if !attribute.path().is_ident("default_method") {
            return true;
        }

        if !matches!(attribute.meta, Meta::Path(_)) {
            error = Some(syn::Error::new_spanned(
                attribute,
                "`default_method` does not accept arguments",
            ));
        } else if marker.is_some() {
            error = Some(syn::Error::new_spanned(
                attribute,
                "duplicate `default_method` marker",
            ));
        } else {
            marker = Some(attribute.span());
        }
        false
    });
    if let Some(error) = error {
        return Err(error);
    }
    Ok(marker.is_some())
}

fn default_method(method: &TraitItemFn) -> syn::Result<TraitItemFn> {
    let mut method = method.clone();
    let Some(receiver_index) = method
        .sig
        .inputs
        .iter()
        .position(|argument| matches!(argument, FnArg::Receiver(_)))
    else {
        return Ok(method);
    };

    let receiver_name = unused_receiver_name(&method);
    let FnArg::Receiver(receiver) = &method.sig.inputs[receiver_index] else {
        unreachable!();
    };
    let attributes = receiver.attrs.clone();
    let binding_mutability = receiver.mutability;
    let (receiver_type, output_lifetime) = receiver_type(receiver, &method)?;

    if let Some(output_lifetime) = output_lifetime {
        if !method
            .sig
            .generics
            .lifetimes()
            .any(|parameter| parameter.lifetime == output_lifetime)
        {
            method
                .sig
                .generics
                .params
                .insert(0, GenericParam::Lifetime(parse_quote!(#output_lifetime)));
        }
        if let ReturnType::Type(_, output) = &mut method.sig.output {
            ElidedOutputLifetimes(&output_lifetime).visit_type_mut(output);
        }
    }

    method.sig.inputs[receiver_index] =
        parse_quote!(#(#attributes)* #binding_mutability #receiver_name: #receiver_type);

    let body = method
        .default
        .take()
        .expect("only provided methods are copied");
    let rewritten = replace_self(quote!(#body), &receiver_name);
    method.default = Some(syn::parse2(rewritten)?);
    Ok(method)
}

fn receiver_type(
    receiver: &syn::Receiver,
    method: &TraitItemFn,
) -> syn::Result<(Type, Option<Lifetime>)> {
    let receiver_type = match &receiver.kind {
        ReceiverKind::Value => (parse_quote!(Self), None),
        ReceiverKind::Reference(_, lifetime, mutability) => {
            let lifetime = lifetime
                .clone()
                .unwrap_or_else(|| unused_receiver_lifetime(method));
            (parse_quote!(&#lifetime #mutability Self), Some(lifetime))
        }
        ReceiverKind::Typed(_, receiver_type) => {
            let output_lifetime = receiver_reference_lifetime(receiver_type, method);
            let mut receiver_type = (**receiver_type).clone();
            if let (Type::Reference(reference), Some(lifetime)) =
                (&mut receiver_type, &output_lifetime)
                && reference.lifetime.is_none()
            {
                reference.lifetime = Some(lifetime.clone());
            }
            (receiver_type, output_lifetime)
        }
        _ => {
            return Err(syn::Error::new_spanned(
                receiver,
                "unsupported trait method receiver",
            ));
        }
    };
    Ok(receiver_type)
}

fn receiver_reference_lifetime(receiver_type: &Type, method: &TraitItemFn) -> Option<Lifetime> {
    let Type::Reference(reference) = receiver_type else {
        return None;
    };
    let Type::Path(path) = reference.elem.as_ref() else {
        return None;
    };
    if !path.path.is_ident("Self") {
        return None;
    }
    Some(
        reference
            .lifetime
            .clone()
            .unwrap_or_else(|| unused_receiver_lifetime(method)),
    )
}

struct ElidedOutputLifetimes<'a>(&'a Lifetime);

impl VisitMut for ElidedOutputLifetimes<'_> {
    fn visit_type_reference_mut(&mut self, reference: &mut TypeReference) {
        if reference.lifetime.is_none() {
            reference.lifetime = Some(self.0.clone());
        }
        visit_mut::visit_type_reference_mut(self, reference);
    }

    fn visit_type_fn_ptr_mut(&mut self, _function: &mut syn::TypeFnPtr) {}
}

fn replace_self(tokens: TokenStream, replacement: &Ident) -> TokenStream {
    tokens
        .into_iter()
        .map(|token| match token {
            TokenTree::Ident(identifier) if identifier == "self" => {
                TokenTree::Ident(TokenIdent::new(&replacement.to_string(), identifier.span()))
            }
            TokenTree::Group(group) => {
                let mut rewritten =
                    Group::new(group.delimiter(), replace_self(group.stream(), replacement));
                rewritten.set_span(group.span());
                TokenTree::Group(rewritten)
            }
            token => token,
        })
        .collect()
}

fn unused_receiver_name(method: &TraitItemFn) -> Ident {
    let tokens = quote!(#method);
    let mut candidate = "__default_methods_receiver".to_owned();
    while contains_ident(tokens.clone(), &candidate) {
        candidate.push('_');
    }
    Ident::new(&candidate, Span::call_site())
}

fn unused_receiver_lifetime(method: &TraitItemFn) -> Lifetime {
    let tokens = quote!(#method);
    let mut candidate = "__default_methods_receiver".to_owned();
    while contains_ident(tokens.clone(), &candidate) {
        candidate.push('_');
    }
    Lifetime::new(&format!("'{candidate}"), Span::call_site())
}

fn contains_ident(tokens: TokenStream, needle: &str) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(identifier) => identifier == needle,
        TokenTree::Group(group) => contains_ident(group.stream(), needle),
        _ => false,
    })
}

fn forwarding_body(
    method: &mut TraitItemFn,
    defaults_name: &Ident,
    trait_arguments: &syn::TypeGenerics<'_>,
) -> syn::Result<syn::Block> {
    let mut arguments = Vec::new();
    let method_tokens = quote!(#method);
    for (index, input) in method.sig.inputs.iter_mut().enumerate() {
        match input {
            FnArg::Receiver(_) => arguments.push(quote!(self)),
            FnArg::Typed(argument) => {
                let name = match argument.pat.as_mut() {
                    Pat::Ident(pattern)
                        if pattern.subpat.is_none()
                            && pattern.by_ref.is_none()
                            && !pattern.ident.to_string().starts_with('_') =>
                    {
                        pattern.mutability = None;
                        pattern.ident.clone()
                    }
                    pattern => {
                        let name = unused_argument_name(&method_tokens, index);
                        *pattern = parse_quote!(#name);
                        name
                    }
                };
                arguments.push(quote!(#name));
            }
        }
    }

    let method_name = &method.sig.ident;
    let method_arguments: Vec<_> = method
        .sig
        .generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            GenericParam::Type(parameter) => Some(&parameter.ident),
            GenericParam::Const(parameter) => Some(&parameter.ident),
            GenericParam::Lifetime(_) => None,
        })
        .collect();
    let turbofish = (!method_arguments.is_empty()).then(|| quote!(::<#(#method_arguments),*>));

    let call = quote! {
        <Self as #defaults_name #trait_arguments>::#method_name #turbofish (#(#arguments),*)
    };
    let call = if matches!(method.sig.safety, syn::Safety::Unsafe(_)) {
        quote!(unsafe { #call })
    } else {
        call
    };
    let call = if method.sig.asyncness.is_some() {
        quote!(#call.await)
    } else {
        call
    };

    syn::parse2(quote!({ #call }))
}

fn unused_argument_name(tokens: &TokenStream, index: usize) -> Ident {
    let mut candidate = format!("default_methods_argument_{index}");
    while contains_ident(tokens.clone(), &candidate) {
        candidate.push('_');
    }
    Ident::new(&candidate, Span::call_site())
}

fn unused_implementor_name(generics: &syn::Generics) -> Ident {
    let mut candidate = "__DefaultMethodsImplementor".to_owned();
    while generics.params.iter().any(|parameter| match parameter {
        GenericParam::Type(parameter) => parameter.ident == candidate,
        GenericParam::Const(parameter) => parameter.ident == candidate,
        GenericParam::Lifetime(_) => false,
    }) {
        candidate.push('_');
    }
    Ident::new(&candidate, Span::call_site())
}
