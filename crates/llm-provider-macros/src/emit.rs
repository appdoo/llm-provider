//! The `impl Tool` both macros emit: the definition from a
//! [`ToolSignature`], and a `call` that parses the input, runs the body and
//! turns its `Result` into a `ToolOutput`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use crate::signature::ToolSignature;

/// The lint allows every `#[tool]` function carries: the generated `call`
/// awaits it, so it is `async` even when its body awaits nothing.
/// `unused_async_trait_impl` is new in clippy 1.99; `unknown_lints` keeps
/// older toolchains quiet about it.
pub(crate) fn allow_sync_body() -> syn::Attribute {
    syn::parse_quote!(#[allow(
        unknown_lints,
        clippy::unused_async,
        clippy::unused_async_trait_impl
    )])
}

/// `impl ::llm_provider::agent::Tool for #target`, where `run` is an
/// expression that runs the tool body: it reads the parsed input from a
/// binding named `input` when the tool takes one, the call context from
/// `context` when it reads it, and evaluates to the body's future.
pub(crate) fn impl_tool(target: &Ident, sig: &ToolSignature, run: &TokenStream) -> TokenStream {
    let name = &sig.name;
    let description = &sig.description;
    let (schema, parse) = schema_and_parse(sig);
    let ignore = if sig.takes_context {
        TokenStream::new()
    } else {
        quote!(let _ = context;)
    };

    quote! {
        #[::llm_provider::__private::async_trait]
        impl ::llm_provider::agent::Tool for #target {
            fn definition(&self) -> ::llm_provider::provider::ToolDefinition {
                ::llm_provider::provider::ToolDefinition::new(#name, #description, #schema)
            }

            async fn call(
                &self,
                input: ::llm_provider::__private::serde_json::Value,
                context: &::llm_provider::agent::CallContext<'_>,
            ) -> ::llm_provider::agent::ToolOutput {
                #parse
                #ignore
                ::llm_provider::__private::output(#run.await)
            }
        }
    }
}

/// The input schema expression of `sig`, and the statements that rebind
/// `input` to the parsed argument (or discard it for a tool without one).
fn schema_and_parse(sig: &ToolSignature) -> (TokenStream, TokenStream) {
    match &sig.input {
        Some(ty) => (
            quote!(::llm_provider::__private::input_schema::<#ty>()),
            quote! {
                let input = match ::llm_provider::__private::parse_input::<#ty>(input) {
                    ::core::result::Result::Ok(input) => input,
                    ::core::result::Result::Err(output) => return output,
                };
            },
        ),
        None => (
            quote!(::llm_provider::__private::empty_schema()),
            quote!(let _ = input;),
        ),
    }
}

/// The arguments a tool body is called with: the parsed `input` when the
/// tool takes one, then `context` when it reads the call context.
pub(crate) fn input_arg(sig: &ToolSignature) -> TokenStream {
    let input = sig.input.as_ref().map(|_| quote!(input));
    let context = sig.takes_context.then(|| quote!(context));
    let args = input.into_iter().chain(context);
    quote!(#(#args),*)
}
