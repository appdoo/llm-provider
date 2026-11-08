//! `#[tool]` on an `async fn`: the function stays as written, and a unit
//! struct named after it in CamelCase implements `Tool` by calling it.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::{Attribute, Ident, ItemFn, LitStr};

use crate::emit::{allow_sync_body, impl_tool, input_arg};
use crate::signature::{rename_parser, ToolSignature};

/// The rule a `#[tool]` method outside a `#[tools]` block breaks.
const METHOD_OUTSIDE_BLOCK: &str = "a #[tool] method belongs in a #[tools] impl block";

/// The expansion of `#[tool(args)]` on `item`, or the compile error that
/// names the rule it breaks.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let mut rename: Option<LitStr> = None;
    rename_parser(&mut rename).parse2(args)?;
    let function: ItemFn = syn::parse2(item)?;

    let sig = ToolSignature::parse(&function.sig, &function.attrs, rename)?;
    if sig.has_receiver {
        return Err(syn::Error::new_spanned(&function.sig, METHOD_OUTSIDE_BLOCK));
    }
    Ok(unit_struct_tool(&function, &sig))
}

/// `function` with the [`allow_sync_body`] lints, the unit struct named
/// after it in CamelCase, and the struct's `impl Tool` that calls `function`.
fn unit_struct_tool(function: &ItemFn, sig: &ToolSignature) -> TokenStream {
    let function_ident = &function.sig.ident;
    let struct_ident = camel_case(function_ident);
    let vis = &function.vis;
    let attrs = function.attrs.iter().filter(|attr| is_doc_or_cfg(attr));

    let arg = input_arg(sig);
    let run = quote!(#function_ident(#arg));
    let tool = impl_tool(&struct_ident, sig, &run);
    let allow = allow_sync_body();

    quote! {
        #allow
        #function

        #(#attrs)*
        #[derive(Debug, Clone, Copy, Default)]
        #vis struct #struct_ident;

        #tool
    }
}

/// `ident` in CamelCase (`unix_time` → `UnixTime`), spanned at `ident`.
fn camel_case(ident: &Ident) -> Ident {
    let name = ident.to_string();
    let name = name.trim_start_matches("r#");
    let camel: String = name
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect();
    format_ident!("{}", camel, span = ident.span())
}

/// Whether `attr` is a doc comment or a `cfg`, the only attributes the
/// struct takes from the function.
fn is_doc_or_cfg(attr: &Attribute) -> bool {
    attr.path().is_ident("doc") || attr.path().is_ident("cfg")
}
