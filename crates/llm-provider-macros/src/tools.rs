//! `#[tools]` on an impl block: every method marked `#[tool]` becomes a
//! tool, and the block's type implements `IntoTools`, its tools sharing one
//! `Arc` of it.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Attribute, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr};

use crate::emit::{allow_sync_body, impl_tool, input_arg};
use crate::signature::{rename_parser, ToolSignature};

/// The rule a `#[tool]` method without `&self` breaks.
const NEEDS_SELF: &str = "a #[tool] method in a #[tools] block takes &self";

/// The rule a generic `#[tools]` block breaks.
const NO_GENERICS: &str = "a #[tools] impl block takes no generic parameters";

/// A `#[tool]` method of the block: its name and checked signature.
struct ToolMethod {
    method: Ident,
    sig: ToolSignature,
}

/// The expansion of `#[tools]` on `item`: the block without its `#[tool]`
/// attributes, a hidden struct and `impl Tool` per tool method, and
/// `impl IntoTools` for the block's type.
pub(crate) fn expand(item: TokenStream) -> syn::Result<TokenStream> {
    let mut block: ItemImpl = syn::parse2(item)?;
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&block.generics, NO_GENERICS));
    }

    let mut methods = Vec::new();
    for item in &mut block.items {
        let ImplItem::Fn(function) = item else {
            continue;
        };
        if let Some(method) = take_tool(function)? {
            methods.push(method);
        }
    }

    let self_ty = &block.self_ty;
    let tools = methods.iter().map(|method| method_tool(self_ty, method));
    let into_tools = impl_into_tools(self_ty, &methods);
    Ok(quote! {
        #block

        const _: () = {
            #(#tools)*
            #into_tools
        };
    })
}

/// The tool behind `function` when it carries `#[tool]`, which is replaced
/// by the [`allow_sync_body`] lints; `None` for an ordinary method.
fn take_tool(function: &mut ImplItemFn) -> syn::Result<Option<ToolMethod>> {
    let Some(index) = function.attrs.iter().position(is_tool_attr) else {
        return Ok(None);
    };
    let attr = function.attrs.remove(index);
    let rename = rename_of(&attr)?;

    let sig = ToolSignature::parse(&function.sig, &function.attrs, rename)?;
    if !sig.has_receiver {
        return Err(syn::Error::new_spanned(&function.sig, NEEDS_SELF));
    }
    function.attrs.push(allow_sync_body());
    Ok(Some(ToolMethod {
        method: function.sig.ident.clone(),
        sig,
    }))
}

/// Whether `attr` is `#[tool]` or `#[tool(…)]`, by its last path segment.
fn is_tool_attr(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "tool")
}

/// The `name = "…"` of a `#[tool(…)]` attribute.
fn rename_of(attr: &Attribute) -> syn::Result<Option<LitStr>> {
    let mut rename = None;
    if let syn::Meta::List(_) = attr.meta {
        attr.parse_args_with(rename_parser(&mut rename))?;
    }
    Ok(rename)
}

/// The hidden struct holding the shared `Arc` for `method`, and its
/// `impl Tool`.
fn method_tool(self_ty: &syn::Type, method: &ToolMethod) -> TokenStream {
    let target = method_struct(&method.method);
    let name = &method.method;
    let arg = input_arg(&method.sig);
    let run = quote!(self.0.#name(#arg));
    let tool = impl_tool(&target, &method.sig, &run);

    quote! {
        #[allow(non_camel_case_types)]
        struct #target(::std::sync::Arc<#self_ty>);

        #tool
    }
}

/// `impl IntoTools` for the block's type: one `Arc` of it, cloned into
/// every tool, in source order.
fn impl_into_tools(self_ty: &syn::Type, methods: &[ToolMethod]) -> TokenStream {
    let targets = methods.iter().map(|method| method_struct(&method.method));
    quote! {
        impl ::llm_provider::agent::IntoTools for #self_ty {
            fn into_tools(
                self,
            ) -> ::std::vec::Vec<::std::boxed::Box<dyn ::llm_provider::agent::Tool>> {
                let shared = ::std::sync::Arc::new(self);
                ::std::vec![
                    #(::std::boxed::Box::new(#targets(::std::sync::Arc::clone(&shared)))
                        as ::std::boxed::Box<dyn ::llm_provider::agent::Tool>),*
                ]
            }
        }
    }
}

/// The identifier of the hidden struct behind the tool method `method`.
fn method_struct(method: &Ident) -> Ident {
    Ident::new(&format!("__Tool_{method}"), Span::call_site())
}
