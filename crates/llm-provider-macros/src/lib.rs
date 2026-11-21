//! The `#[tool]` and `#[tools]` attribute macros of `llm-provider`. Use
//! them through `llm_provider::agent::{tool, tools}`; the code they emit
//! names `::llm_provider` paths only.
//!
//! ```ignore
//! use llm_provider::agent::{tool, Tools};
//!
//! /// Current time in seconds since 1970-01-01 UTC.
//! #[tool]
//! async fn unix_time() -> Result<String, std::io::Error> {
//!     Ok("0".into())
//! }
//!
//! let tools = Tools::new().with(UnixTime);
//! ```

#![warn(clippy::pedantic)]
#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod emit;
mod signature;
mod tool;
mod tools;

/// Turns an `async fn` into a tool: the function stays as written, and a
/// unit struct named after it in CamelCase (`unix_time` → `UnixTime`)
/// implements `llm_provider::agent::Tool`.
///
/// The function takes no argument or one struct that derives `Deserialize`
/// and `JsonSchema`, optionally followed by a last `&CallContext<'_>`
/// parameter that receives the calling conversation, the turn's cancel
/// token and its number, and returns
/// `Result<impl Display, impl Display>`. Its
/// doc comment is the tool's description; its name is the tool's name unless
/// `#[tool(name = "…")]` sets another.
#[proc_macro_attribute]
pub fn tool(args: TokenStream, item: TokenStream) -> TokenStream {
    tool::expand(args.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Turns every `#[tool]` method of an impl block into a tool, and implements
/// `llm_provider::agent::IntoTools` for the block's type.
///
/// Each tool method takes `&self` and follows the rules of [`macro@tool`].
/// The value is moved into one `Arc` that every tool of the block shares.
/// Methods without `#[tool]` stay ordinary methods.
#[proc_macro_attribute]
pub fn tools(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = proc_macro2::TokenStream::from(args);
    if !args.is_empty() {
        return syn::Error::new_spanned(args, "#[tools] takes no arguments")
            .into_compile_error()
            .into();
    }
    tools::expand(item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
