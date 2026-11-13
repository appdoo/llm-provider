//! K-2, K-3: outside the crate, the frozen structs cannot be built as
//! literals and the frozen enums cannot be matched without a wildcard arm.

#[test]
fn frozen_shapes_fail_to_compile() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
