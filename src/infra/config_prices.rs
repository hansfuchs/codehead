use super::{BTreeMap, File, path, read_layers};

/// `[prices."<model>"]`: $ per 1M tokens of a model, for harnesses that report 0 $ (Codex, most
/// ACP agents). 0 built-in prices: a model without an entry costs 0 $.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Price {
    pub input: f64,
    /// Cache reads; unset: `input`.
    pub cached: f64,
    pub output: f64,
    /// Cache writes; unset: `input`.
    pub cache_write: f64,
}

// parse refuses NaN and infinities: equality of 2 prices is total
impl Eq for Price {}

/// Tokens of 1 turn, by the price each one has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tokens {
    /// Input without cache reads and writes.
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Price {
    /// $ of TOKENS.
    pub(crate) fn usd(&self, tokens: Tokens) -> f64 {
        let per_token = |count: u64, per_million: f64| count as f64 * per_million / 1_000_000.0;

        per_token(tokens.input, self.input)
            + per_token(tokens.cached, self.cached)
            + per_token(tokens.cache_write, self.cache_write)
            + per_token(tokens.output, self.output)
    }
}

const PRICE_FIELDS: [&str; 4] = ["input", "cached", "output", "cache_write"];

/// The `[prices]` tables of the files, read now (0 tables or a broken file: none).
pub(crate) fn prices() -> BTreeMap<String, Price> {
    read_layers(&path())
        .map(|layers| layers.merged().prices)
        .unwrap_or_default()
}

/// The price of MODEL in PRICES: its own entry, else the entry of its name without the provider
/// (`openai/gpt-x` → `gpt-x`).
pub(crate) fn price_of(prices: &BTreeMap<String, Price>, model: &str) -> Option<Price> {
    let model = model.trim();

    prices
        .get(model)
        .or_else(|| prices.get(model.rsplit('/').next()?))
        .copied()
}

pub(super) fn parse_prices(file: &mut File, value: &toml::Value) -> Result<(), String> {
    for (model, table) in value.as_table().ok_or("prices: must be a table")? {
        let table = table
            .as_table()
            .ok_or_else(|| format!("prices.{model}: must be a table"))?;
        let field = |name: &str| price_field(model, name, table.get(name));

        if let Some(unknown) = table.keys().find(|key| !PRICE_FIELDS.contains(&key.as_str())) {
            return Err(format!("unknown key prices.{model}.{unknown}"));
        }

        let (Some(input), Some(output)) = (field("input")?, field("output")?) else {
            return Err(format!("prices.{model}: input and output required ($ per 1M tokens)"));
        };

        let price = Price {
            input,
            cached: field("cached")?.unwrap_or(input),
            output,
            cache_write: field("cache_write")?.unwrap_or(input),
        };

        file.prices.insert(model.clone(), price);
    }

    Ok(())
}

/// Field NAME of the price of MODEL: $ per 1M tokens, a number ≥ 0.
fn price_field(model: &str, name: &str, value: Option<&toml::Value>) -> Result<Option<f64>, String> {
    let Some(value) = value else { return Ok(None) };

    let number = match value {
        toml::Value::Float(number) => *number,
        toml::Value::Integer(number) => *number as f64,
        _ => f64::NAN,
    };

    if !number.is_finite() || number < 0.0 {
        return Err(format!("prices.{model}.{name}: $ per 1M tokens, a number ≥ 0"));
    }

    Ok(Some(number))
}
