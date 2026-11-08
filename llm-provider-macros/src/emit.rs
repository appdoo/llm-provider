//! The `impl Tool` both macros emit: the definition from a
//! [`ToolSignature`], and a `call` that parses the input, runs the body and
//! turns its `Result` into a `ToolOutput`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use crate::signature::ToolSignature;

/// `impl ::llm_provider::agent::Tool for #target`, where `run` is an
/// expression that runs the tool body: it reads the parsed input from a
/// binding named `input` when the tool takes one, and evaluates to the
/// body's future.
pub(crate) fn impl_tool(target: &Ident, sig: &ToolSignature, run: &TokenStream) -> TokenStream {
    let name = &sig.name;
    let description = &sig.description;
    let (schema, parse) = schema_and_parse(sig);

    quote! {
        #[::llm_provider::__private::async_trait]
        impl ::llm_provider::agent::Tool for #target {
            fn definition(&self) -> ::llm_provider::provider::ToolDefinition {
                ::llm_provider::provider::ToolDefinition {
                    name: ::std::string::String::from(#name),
                    description: ::std::string::String::from(#description),
                    input_schema: #schema,
                }
            }

            async fn call(
                &self,
                input: ::llm_provider::__private::serde_json::Value,
            ) -> ::llm_provider::agent::ToolOutput {
                #parse
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

/// The argument a tool body is called with: the parsed `input` when the
/// tool takes one, nothing otherwise.
pub(crate) fn input_arg(sig: &ToolSignature) -> TokenStream {
    if sig.input.is_some() {
        quote!(input)
    } else {
        TokenStream::new()
    }
}
