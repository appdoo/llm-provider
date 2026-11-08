use syn::parse::Parser;
use syn::{parse_quote, ItemFn};

use super::*;

/// The signature of `function`, renamed by `rename` when given.
fn parse(function: &ItemFn, rename: Option<&str>) -> syn::Result<ToolSignature> {
    let rename = rename.map(|name| LitStr::new(name, proc_macro2::Span::call_site()));
    ToolSignature::parse(&function.sig, &function.attrs, rename)
}

#[test]
fn description_joins_doc_lines_with_a_space() {
    let function: ItemFn = parse_quote! {
        /// Returns some lines
        ///
        ///   of a text.
        async fn lines(range: LineRange) -> Result<String, Infallible> { todo!() }
    };

    let sig = parse(&function, None).unwrap();

    assert_eq!(sig.name, "lines");
    assert_eq!(sig.description, "Returns some lines of a text.");
    assert!(sig.input.is_some());
    assert!(!sig.has_receiver);
}

#[test]
fn rename_overrides_the_function_name() {
    let function: ItemFn = parse_quote! {
        /// Now.
        async fn unix_time() -> Result<String, Infallible> { todo!() }
    };

    let sig = parse(&function, Some("now")).unwrap();

    assert_eq!(sig.name, "now");
    assert!(sig.input.is_none());
}

#[test]
fn each_rule_names_itself() {
    let undocumented: ItemFn = parse_quote!(
        async fn a() -> Result<(), ()> {}
    );
    let blocking: ItemFn = parse_quote! {
        /// B.
        fn b() -> Result<(), ()> {}
    };
    let two: ItemFn = parse_quote! {
        /// C.
        async fn c(x: X, y: Y) -> Result<(), ()> {}
    };

    let message = |function: &ItemFn| parse(function, None).err().unwrap().to_string();
    assert_eq!(message(&undocumented), NEEDS_DOC);
    assert_eq!(message(&blocking), NEEDS_ASYNC);
    assert_eq!(message(&two), ONE_ARGUMENT);
}

#[test]
fn rename_parser_reads_name_and_rejects_others() {
    let mut rename = None;
    rename_parser(&mut rename)
        .parse2(quote::quote!(name = "now"))
        .unwrap();
    assert_eq!(rename.unwrap().value(), "now");

    let mut other = None;
    let error = rename_parser(&mut other)
        .parse2(quote::quote!(title = "now"))
        .unwrap_err();
    assert!(error.to_string().contains("name"), "{error}");
}
