//! [`Prices`]: dollars per million tokens, to turn a [`Usage`](crate::Usage)
//! into a cost.

/// Dollars per million tokens for one model; [`Usage::cost`](crate::Usage::cost)
/// multiplies them with the token counts.
///
/// The crate ships no price table, because prices change; you name them.
/// [`per_million`](Self::per_million) takes the input and output price and
/// derives the cache prices from the input price the way Anthropic prices
/// them: a write for five minutes 1.25 times, a write for one hour 2 times, a
/// read 0.1 times. Each can be overridden.
///
/// Amazon Bedrock bills the regional inference profiles (`eu.`, `us.`,
/// `apac.`) at 1.10 times the global list price, in every category; `global.`
/// profiles and plain model ids cost 1.0 times. [`regional`](Self::regional)
/// applies the factor. This rule was verified against an AWS bill (tokens from
/// `CloudWatch` matched to 0.0%). Take the factor from the model id you
/// configured: Bedrock echoes the bare model name, which has lost the prefix.
///
/// A [`Usage`](crate::Usage) is per call, and a turn's
/// [`TurnSummary::usage`] adds its calls up; both are deltas, never running
/// totals, so their costs add too.
///
/// ```
/// use llm_provider::{Prices, Usage};
///
/// // Claude Sonnet, Anthropic list prices of 2026-11-21 (dollars per million),
/// // on Bedrock through an `eu.` inference profile: list price × 1.10.
/// let sonnet = Prices::per_million(3.0, 15.0).regional(1.10);
///
/// let usage = Usage::default()
///     .input_tokens(2_000)
///     .output_tokens(500);
/// let dollars = usage.cost(&sonnet);
/// assert!((dollars - 0.01485).abs() < 1e-9);
/// ```
///
/// [`TurnSummary::usage`]: crate::agent::TurnSummary::usage
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prices {
    pub(crate) input: f64,
    pub(crate) output: f64,
    pub(crate) cache_write_5m: f64,
    pub(crate) cache_write_1h: f64,
    pub(crate) cache_read: f64,
}

impl Prices {
    /// Prices for `input` and `output` dollars per million tokens, with the
    /// cache prices derived from `input`: write 1.25 times (five minutes) or 2
    /// times (one hour), read 0.1 times.
    #[must_use]
    pub fn per_million(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_write_5m: input * 1.25,
            cache_write_1h: input * 2.0,
            cache_read: input * 0.1,
        }
    }

    /// Multiplies every price by `factor`, for example 1.10 for a Bedrock
    /// `eu.`, `us.` or `apac.` inference profile.
    #[must_use]
    pub fn regional(self, factor: f64) -> Self {
        Self {
            input: self.input * factor,
            output: self.output * factor,
            cache_write_5m: self.cache_write_5m * factor,
            cache_write_1h: self.cache_write_1h * factor,
            cache_read: self.cache_read * factor,
        }
    }

    /// Sets the price of a cache write with the five-minute lifetime.
    #[must_use]
    pub fn cache_write_5m(self, price: f64) -> Self {
        Self {
            cache_write_5m: price,
            ..self
        }
    }

    /// Sets the price of a cache write with the one-hour lifetime.
    #[must_use]
    pub fn cache_write_1h(self, price: f64) -> Self {
        Self {
            cache_write_1h: price,
            ..self
        }
    }

    /// Sets the price of a cache read.
    #[must_use]
    pub fn cache_read(self, price: f64) -> Self {
        Self {
            cache_read: price,
            ..self
        }
    }

    /// The dollars for `lines`, each a token count and its price per million.
    pub(crate) fn dollars(lines: &[(u32, f64)]) -> f64 {
        lines
            .iter()
            .map(|&(tokens, price)| f64::from(tokens) * price / 1_000_000.0)
            .sum()
    }
}
