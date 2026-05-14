use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Three-level trust discriminator (spec §4.7).
///
/// Quarantines externally-sourced strings the model reads so prompt-injection
/// attempts surface as data, not instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// OS-API output (window class, AT-SPI role enum, geometry, mode).
    Trusted,
    /// Content the agent recently authored (clipboard we set; file we created).
    AgentAuthored,
    /// Externally-sourced text (window title, AT-SPI name/value/description,
    /// clipboard read we didn't write, OCR text, bash stdout, file content).
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Tagged<T: JsonSchema> {
    pub trust: Trust,
    pub text: T,
}

impl<T: JsonSchema> Tagged<T> {
    pub fn trusted(text: T) -> Self {
        Self {
            trust: Trust::Trusted,
            text,
        }
    }
    pub fn agent_authored(text: T) -> Self {
        Self {
            trust: Trust::AgentAuthored,
            text,
        }
    }
    pub fn external(text: T) -> Self {
        Self {
            trust: Trust::External,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_trust_and_text_fields() {
        let s = Tagged::external("Save File".to_string());
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["trust"], "external");
        assert_eq!(v["text"], "Save File");
    }

    #[test]
    fn round_trip_through_serde() {
        let s = Tagged::trusted("PushButton".to_string());
        let json = serde_json::to_string(&s).unwrap();
        let back: Tagged<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn all_three_trust_levels_have_distinct_serializations() {
        let t = serde_json::to_value(Trust::Trusted).unwrap();
        let a = serde_json::to_value(Trust::AgentAuthored).unwrap();
        let e = serde_json::to_value(Trust::External).unwrap();
        assert_eq!(t, "trusted");
        assert_eq!(a, "agent_authored");
        assert_eq!(e, "external");
    }
}
