//! M-7: a `#[tool]` that breaks a rule fails to compile with a message
//! naming the rule.

#[test]
fn rule_breaking_tools_fail_to_compile() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
