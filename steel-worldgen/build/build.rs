#![allow(
    dead_code,
    missing_docs,
    reason = "build-script modules include parser shapes and generated-code helpers"
)]

use std::{env, fs, path::Path};

use steel_utils as _;

mod density;
mod multi_noise;
mod noise_parameters;

const MULTI_NOISE: &str = "multi_noise";
const NOISE_PARAMETERS: &str = "noise_parameters";

pub fn main() {
    println!("cargo:rerun-if-changed=build/");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = Path::new(&manifest_dir).join("src/generated");
    fs::create_dir_all(&out_dir).expect("failed to create worldgen generated directory");

    let generated_files = [
        (multi_noise::build(), MULTI_NOISE),
        (noise_parameters::build(), NOISE_PARAMETERS),
    ];

    for (content, file_name) in generated_files {
        let path = out_dir.join(format!("vanilla_{file_name}.rs"));
        write_if_changed(&path, content);
    }

    let df = density::build();
    let df_dir = out_dir.join("vanilla_density_functions");
    fs::create_dir_all(&df_dir).expect("failed to create generated density function directory");

    for (content, name) in [
        (df.overworld, "overworld"),
        (df.nether, "nether"),
        (df.end, "end"),
    ] {
        let path = df_dir.join(format!("{name}.rs"));
        write_if_changed(&path, content);
    }

    let path = df_dir.join("mod.rs");
    write_if_changed(&path, df.index);
}

fn write_if_changed(path: &Path, tokens: proc_macro2::TokenStream) {
    // Parse the tokens directly instead of printing them to a string and parsing that again.
    #[cfg(feature = "fmt")]
    let content = {
        let syntax: syn::File = syn::parse2(tokens).unwrap_or_else(|error| {
            panic!("Failed to parse generated file {}: {error}", path.display())
        });
        prettyplease::unparse(&syntax)
    };
    // Without `fmt`, add line breaks, because rustc is slow on very long lines.
    #[cfg(not(feature = "fmt"))]
    let content = {
        let mut out = String::new();
        print_with_breaks(tokens, &mut out);
        out
    };
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
