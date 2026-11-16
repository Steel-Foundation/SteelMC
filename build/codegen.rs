//! Shared formatting and file output for generated Rust build assets.

use std::{fs, path::Path};

pub fn write_tokens_if_changed(path: impl AsRef<Path>, tokens: proc_macro2::TokenStream) {
    let path = path.as_ref();
    #[cfg(feature = "fmt")]
    let content = {
        let syntax: syn::File = syn::parse2(tokens).unwrap_or_else(|error| {
            panic!("Failed to parse generated file {}: {error}", path.display())
        });
        prettyplease::unparse(&syntax)
    };
    // Line breaks avoid rustc's slow handling of very long lines without full formatting.
    #[cfg(not(feature = "fmt"))]
    let content = {
        let mut out = String::new();
        print_with_breaks(tokens, &mut out);
        out
    };
    write_file_if_changed(path, content);
}

pub fn write_file_if_changed(path: &Path, content: String) {
    if fs::read_to_string(path).is_ok_and(|existing| existing == content) {
        return;
    }

    if let Err(error) = fs::write(path, content) {
        panic!("Failed to write {}: {error}", path.display());
    }
}

/// Writes tokens like `TokenStream::to_string()`, plus a line break after every `,` and `;`.
#[cfg(not(feature = "fmt"))]
fn print_with_breaks(ts: proc_macro2::TokenStream, out: &mut String) {
    use proc_macro2::{Delimiter, Spacing, TokenTree};
    for tt in ts {
        match tt {
            TokenTree::Group(g) => {
                let (open, close) = match g.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                out.push_str(open);
                print_with_breaks(g.stream(), out);
                out.push_str(close);
                out.push(' ');
            }
            TokenTree::Punct(p) => {
                out.push(p.as_char());
                if p.spacing() == Spacing::Alone {
                    out.push(if matches!(p.as_char(), ',' | ';') {
                        '\n'
                    } else {
                        ' '
                    });
                }
            }
            other => {
                out.push_str(&other.to_string());
                out.push(' ');
            }
        }
    }
}
