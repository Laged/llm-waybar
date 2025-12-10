use llm_bridge_core::provider::UsageMetrics;
use crate::transcript::TokenUsage;

// Claude Sonnet 3.5 pricing (per million tokens)
const INPUT_PRICE: f64 = 3.0;
const OUTPUT_PRICE: f64 = 15.0;
const CACHE_READ_PRICE: f64 = 0.30;
const CACHE_WRITE_PRICE: f64 = 3.75;

pub fn calculate_cost(usages: &[TokenUsage]) -> UsageMetrics {
    let mut total = UsageMetrics::default();

    for usage in usages {
        total.input_tokens += usage.input_tokens;
        total.output_tokens += usage.output_tokens;
        total.cache_read += usage.cache_read_input_tokens;
        total.cache_write += usage.cache_creation_input_tokens;
    }

    total.estimated_cost =
        (total.input_tokens as f64 * INPUT_PRICE / 1_000_000.0) +
        (total.output_tokens as f64 * OUTPUT_PRICE / 1_000_000.0) +
        (total.cache_read as f64 * CACHE_READ_PRICE / 1_000_000.0) +
        (total.cache_write as f64 * CACHE_WRITE_PRICE / 1_000_000.0);

    total
}
