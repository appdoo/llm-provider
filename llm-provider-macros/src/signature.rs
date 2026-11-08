//! [`ToolSignature`]: what a `#[tool]` function or method tells the model,
//! read and checked from its signature, doc comment and attribute.

use syn::{Attribute, Expr, FnArg, Lit, LitStr, Meta, Signature, Type};

/// The rule a `#[tool]` without a doc comment breaks.
const NEEDS_DOC: &str = "a #[tool] needs a doc comment: it is the tool's description";

/// The rule a non-async `#[tool]` breaks.
const NEEDS_ASYNC: &str = "a #[tool] fn must be async";

/// The rule a `#[tool]` with more than one argument breaks.
const ONE_ARGUMENT: &str =
    "a #[tool] fn takes no argument or one struct that derives Deserialize and JsonSchema";

/// The rule a generic `#[tool]` breaks.
const NO_GENERICS: &str = "a #[tool] fn takes no generic parameters";

/// A checked `#[tool]`: its name, description and input type.
pub(crate) struct ToolSignature {
    /// The name the model calls the tool by.
    pub(crate) name: String,
    /// The doc comment, lines trimmed and joined with a space.
    pub(crate) description: String,
    /// The one argument's type, if the tool takes one.
    pub(crate) input: Option<Type>,
    /// Whether the function takes `self`.
    pub(crate) has_receiver: bool,
}

impl ToolSignature {
    /// Reads and checks the tool behind `sig`, documented by `attrs`;
    /// `rename` is the `name = "…"` of its `#[tool(…)]`, if given.
    pub(crate) fn parse(
        sig: &Signature,
        attrs: &[Attribute],
        rename: Option<LitStr>,
    ) -> syn::Result<Self> {
        if sig.asyncness.is_none() {
            return Err(syn::Error::new_spanned(sig.fn_token, NEEDS_ASYNC));
        }
        if !sig.generics.params.is_empty() {
            return Err(syn::Error::new_spanned(&sig.generics, NO_GENERICS));
        }
        let Some(description) = description(attrs) else {
            return Err(syn::Error::new_spanned(&sig.ident, NEEDS_DOC));
        };

        let has_receiver = sig.receiver().is_some();
        let input = input_type(sig)?;
        let name = rename.map_or_else(|| sig.ident.to_string(), |name| name.value());
        Ok(Self {
            name,
            description,
            input,
            has_receiver,
        })
    }
}

/// The type of the one non-`self` argument of `sig`, if there is one.
fn input_type(sig: &Signature) -> syn::Result<Option<Type>> {
    let mut typed = sig.inputs.iter().filter_map(|arg| match arg {
        FnArg::Typed(pat) => Some(pat),
        FnArg::Receiver(_) => None,
    });

    let first = typed.next();
    if let Some(extra) = typed.next() {
        return Err(syn::Error::new_spanned(extra, ONE_ARGUMENT));
    }
    Ok(first.map(|pat| (*pat.ty).clone()))
}

/// The doc comment in `attrs`, lines trimmed and joined with a space, or
/// `None` when there is none.
fn description(attrs: &[Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
        .iter()
        .filter_map(doc_line)
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// The text of a `#[doc = "…"]` attribute.
fn doc_line(attr: &Attribute) -> Option<String> {
    let Meta::NameValue(meta) = &attr.meta else {
        return None;
    };
    if !meta.path.is_ident("doc") {
        return None;
    }
    let Expr::Lit(expr) = &meta.value else {
        return None;
    };
    let Lit::Str(text) = &expr.lit else {
        return None;
    };
    Some(text.value())
}

/// A parser for the arguments of `#[tool(…)]` that stores its
/// `name = "…"` in `rename`.
pub(crate) fn rename_parser(
    rename: &mut Option<LitStr>,
) -> impl syn::parse::Parser<Output = ()> + '_ {
    syn::meta::parser(move |meta| {
        if meta.path.is_ident("name") {
            *rename = Some(meta.value()?.parse()?);
            return Ok(());
        }
        Err(meta.error("#[tool] takes only `name = \"…\"`"))
    })
}

#[cfg(test)]
mod tests;
