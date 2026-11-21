//! Opus works, Fable advises. Opus 5.5 fixes a small program in a scratch
//! directory with the file tools; Fable 5.1 reads the whole conversation
//! when Opus calls it, at most twice. Opus's text streams as it arrives;
//! afterwards each piece of advice and a tally are printed.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example advisor --features fs-tools
//! ```

use std::io::Write as _;
use std::path::Path;

use llm_provider::agent::{Advisor, Session, Tools, TurnSummary, CONSULT_ADVISOR};
use llm_provider::fs_tools::{Files, Sandbox};
use llm_provider::Anthropic;

/// The worker's role; `CONSULT_ADVISOR` is appended to it.
const CODER: &str = "You fix code. You can read and edit files but not run them, \
so check your fix by tracing the examples in TASK.md by hand.";

/// What the program should do, with examples that act as its tests.
const TASK: &str = "\
# Task

`stats.py` defines `median(values)`. It must return the median of a non-empty
list of numbers without changing the list.

Expected:
- median([3, 1, 2]) == 2
- median([4, 1, 3, 2]) == 2.5
- median([5]) == 5
- after `xs = [3, 1, 2]; median(xs)`, `xs` is still [3, 1, 2]
";

/// The program, with two bugs: it sorts in place and gets the even case
/// wrong.
const STATS: &str = "\
def median(values):
    values.sort()
    middle = len(values) // 2
    if len(values) % 2 == 0:
        return values[middle]
    return values[middle]
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = api_key();
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("TASK.md"), TASK)?;
    std::fs::write(dir.path().join("stats.py"), STATS)?;

    let opus = Anthropic::new(key.as_str()).llm(Anthropic::CLAUDE_OPUS_5_5);
    let advisor =
        Advisor::new(Anthropic::new(key.as_str()).llm(Anthropic::CLAUDE_FABLE_5_1)).max_uses(2);
    let tools = Tools::new()
        .with_all(Files::new(Sandbox::new(dir.path())?))
        .with(advisor);

    let mut coder = Session::builder(opus, tools)
        .system(format!("{CODER}\n\n{CONSULT_ADVISOR}"))
        .max_iterations(20)
        .on_delta(|text| print!("{text}"))
        .build();
    let turn = coder.run_turn("Make stats.py meet TASK.md.").await?;
    std::io::stdout().flush()?;

    print_advice(&turn);
    print_result(&turn, &dir.path().join("stats.py"))?;
    Ok(())
}

/// Prints every advisor call of the turn with its advice.
fn print_advice(turn: &TurnSummary) {
    for call in turn.tool_calls.iter().filter(|call| call.name == "advisor") {
        println!("\n\n--- advisor ---\n{}", call.output.content);
    }
}

/// Prints the tally and the file as the agent left it.
fn print_result(turn: &TurnSummary, stats: &Path) -> std::io::Result<()> {
    let advice = turn
        .tool_calls
        .iter()
        .filter(|call| call.name == "advisor")
        .count();
    println!(
        "\ntool calls: {}, of them advisor: {advice}",
        turn.tool_calls.len()
    );
    println!("\n--- stats.py ---\n{}", std::fs::read_to_string(stats)?);
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!(
            "usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example advisor --features fs-tools"
        );
        std::process::exit(2)
    })
}
