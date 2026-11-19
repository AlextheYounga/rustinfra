use std::fmt;

/// A value whose clear text must not appear in command displays or diagnostics.
#[derive(Clone, Eq, PartialEq)]
pub struct HiddenValue {
    raw_value: HiddenContent,
    masked_value: String,
}

#[derive(Clone, Eq, PartialEq)]
enum HiddenContent {
    Text(String),
    Hidden(Box<HiddenValue>),
}

impl HiddenValue {
    /// Creates a value displayed as `*MASKED*`.
    pub fn new(content: impl Into<HiddenValue>) -> Self {
        content.into()
    }

    /// Creates a value with a caller-supplied display mask.
    #[must_use]
    pub fn with_mask(content: impl Into<String>, masked_value: impl Into<String>) -> Self {
        Self { raw_value: HiddenContent::Text(content.into()), masked_value: masked_value.into() }
    }

    /// Returns the clear text, recursively unwrapping nested hidden values.
    #[must_use]
    pub fn unmask(&self) -> &str {
        match &self.raw_value {
            HiddenContent::Text(value) => value,
            HiddenContent::Hidden(value) => value.unmask(),
        }
    }

    /// Returns the safe value used in displays and diagnostics.
    #[must_use]
    pub fn masked_value(&self) -> &str {
        &self.masked_value
    }
}

impl From<&str> for HiddenValue {
    fn from(value: &str) -> Self {
        Self { raw_value: HiddenContent::Text(value.to_owned()), masked_value: "*MASKED*".to_owned() }
    }
}

impl From<String> for HiddenValue {
    fn from(value: String) -> Self {
        Self { raw_value: HiddenContent::Text(value), masked_value: "*MASKED*".to_owned() }
    }
}

impl From<&HiddenValue> for HiddenValue {
    fn from(value: &HiddenValue) -> Self {
        Self { raw_value: HiddenContent::Hidden(Box::new(value.clone())), masked_value: "*MASKED*".to_owned() }
    }
}

impl fmt::Display for HiddenValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.masked_value())
    }
}

impl fmt::Debug for HiddenValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("HiddenValue").field(&self.masked_value()).finish()
    }
}

/// A command environment value, which may be safe to display or secret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnvValue {
    String(String),
    Hidden(HiddenValue),
}

impl From<&str> for EnvValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for EnvValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<HiddenValue> for EnvValue {
    fn from(value: HiddenValue) -> Self {
        Self::Hidden(value)
    }
}

#[cfg(test)]
mod tests {
    use super::HiddenValue;

    #[test]
    fn nested_hidden_value_reveals_original_value_without_exposing_it_in_display() {
        let inner = HiddenValue::new("top secret");
        let outer = HiddenValue::new(&inner);

        assert_eq!(outer.unmask(), "top secret");
        assert_eq!(outer.to_string(), "*MASKED*");
        assert!(!format!("{outer:?}").contains("top secret"));
    }
}
