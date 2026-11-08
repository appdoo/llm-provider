//! `#[tool]` on an `async fn`: the function becomes a unit struct of the
//! same name that implements `Tool`, its body an associated function.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::{ItemFn, LitStr};

use crate::emit::{impl_tool, input_arg};
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

/// The unit struct named after `function`, its body as `__tool_body`, and
/// its `impl Tool`.
fn unit_struct_tool(function: &ItemFn, sig: &ToolSignature) -> TokenStream {
    let ItemFn {
        attrs, vis, block, ..
    } = function;
    let ident = &function.sig.ident;
    let inputs = &function.sig.inputs;
    let output = &function.sig.output;

    let arg = input_arg(sig);
    let run = quote!(Self::__tool_body(#arg));
    let tool = impl_tool(ident, sig, &run);

    quote! {
        #(#attrs)*
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone, Copy, Default)]
        #vis struct #ident;

        impl #ident {
            async fn __tool_body(#inputs) #output #block
        }

        #tool
    }
}
