/*!
llm-output-validator: rule-based validator for LLM string outputs.

Compose rules to validate that an LLM response meets requirements before
passing it downstream. Rules include length limits, regex patterns, allowed
value lists, JSON validity, and simple PII checks.

All rules run on every call (there is no short-circuit), so a single
[`Validator::validate`] returns *all* violations, not just the first one.

```rust
use llm_output_validator::{Validator, Rule};

let v = Validator::new(vec![
    Rule::MinLength(5),
    Rule::MaxLength(100),
    Rule::ValidJson,
]);

// A valid response satisfies every rule.
let r = v.validate("{\"ok\": true}");
assert!(r.ok);
assert!(r.violations.is_empty());

// An invalid response collects every violation.
let r = v.validate("no");
assert!(!r.ok);
assert_eq!(r.violations.len(), 2); // too short, and not valid JSON
```
*/

use regex::Regex;
use std::collections::HashSet;

// ---- simple PII patterns --------------------------------------------------

/// Rough PII regex patterns (email, US phone, SSN-shape).
fn pii_patterns() -> Vec<Regex> {
    vec![
        Regex::new(r"(?i)\b[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}\b").unwrap(),
        Regex::new(r"\b\d{3}[-.\s]?\d{2}[-.\s]?\d{4}\b").unwrap(), // SSN
        Regex::new(r"\b(?:\+1\s?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b").unwrap(), // phone
    ]
}

// ---- Rule -----------------------------------------------------------------

/// A single validation rule.
#[derive(Debug, Clone)]
pub enum Rule {
    /// Minimum character length (inclusive).
    MinLength(usize),
    /// Maximum character length (inclusive).
    MaxLength(usize),
    /// The text must match this regex pattern.
    Regex(String),
    /// The text must not match this regex pattern.
    NotRegex(String),
    /// The text must be one of these exact strings.
    AllowedValues(Vec<String>),
    /// The text must not contain rough PII patterns (email, phone, SSN).
    NoPii,
    /// The text must be valid JSON.
    ValidJson,
    /// The text must not be empty or only whitespace.
    NonEmpty,
}

impl Rule {
    /// Check the rule against `text`. Returns a violation message if the rule is violated.
    pub fn check(&self, text: &str) -> Option<String> {
        match self {
            Rule::MinLength(n) => {
                if text.chars().count() < *n {
                    Some(format!(
                        "MinLength: expected >= {} chars, got {}",
                        n,
                        text.chars().count()
                    ))
                } else {
                    None
                }
            }
            Rule::MaxLength(n) => {
                if text.chars().count() > *n {
                    Some(format!(
                        "MaxLength: expected <= {} chars, got {}",
                        n,
                        text.chars().count()
                    ))
                } else {
                    None
                }
            }
            Rule::Regex(pattern) => match Regex::new(pattern) {
                Ok(re) => {
                    if !re.is_match(text) {
                        Some(format!("Regex: text does not match /{}/", pattern))
                    } else {
                        None
                    }
                }
                Err(e) => Some(format!("Regex: invalid pattern: {}", e)),
            },
            Rule::NotRegex(pattern) => match Regex::new(pattern) {
                Ok(re) => {
                    if re.is_match(text) {
                        Some(format!(
                            "NotRegex: text matches forbidden pattern /{}/",
                            pattern
                        ))
                    } else {
                        None
                    }
                }
                Err(e) => Some(format!("NotRegex: invalid pattern: {}", e)),
            },
            Rule::AllowedValues(allowed) => {
                let set: HashSet<&String> = allowed.iter().collect();
                if !set.contains(&text.to_string()) {
                    Some(format!(
                        "AllowedValues: '{}' is not in the allowed set",
                        text
                    ))
                } else {
                    None
                }
            }
            Rule::NoPii => {
                for re in pii_patterns() {
                    if re.is_match(text) {
                        return Some("NoPii: text appears to contain PII".to_owned());
                    }
                }
                None
            }
            Rule::ValidJson => {
                if serde_json::from_str::<serde_json::Value>(text).is_err() {
                    Some("ValidJson: text is not valid JSON".to_owned())
                } else {
                    None
                }
            }
            Rule::NonEmpty => {
                if text.trim().is_empty() {
                    Some("NonEmpty: text is empty or whitespace".to_owned())
                } else {
                    None
                }
            }
        }
    }
}

// ---- ValidationResult -----------------------------------------------------

/// Result of running all rules against a text.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub violations: Vec<String>,
}

// ---- ValidationError -------------------------------------------------------

/// Error raised when validation fails.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub violations: Vec<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ValidationError: {}", self.violations.join("; "))
    }
}

impl std::error::Error for ValidationError {}

// ---- Validator ------------------------------------------------------------

/// Compose multiple rules into a single validator.
pub struct Validator {
    rules: Vec<Rule>,
}

impl Validator {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Run all rules. Returns a `ValidationResult` with all violations.
    pub fn validate(&self, text: &str) -> ValidationResult {
        let violations: Vec<String> = self.rules.iter().filter_map(|r| r.check(text)).collect();
        ValidationResult {
            ok: violations.is_empty(),
            violations,
        }
    }

    /// Return `Err(ValidationError)` if any rule fails.
    pub fn check_or_raise(&self, text: &str) -> Result<(), ValidationError> {
        let result = self.validate(text);
        if result.ok {
            Ok(())
        } else {
            Err(ValidationError {
                violations: result.violations,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_length_pass() {
        let r = Rule::MinLength(3).check("hello");
        assert!(r.is_none());
    }

    #[test]
    fn min_length_fail() {
        let r = Rule::MinLength(10).check("hi");
        assert!(r.is_some());
    }

    #[test]
    fn max_length_pass() {
        let r = Rule::MaxLength(100).check("hello");
        assert!(r.is_none());
    }

    #[test]
    fn max_length_fail() {
        let r = Rule::MaxLength(3).check("hello");
        assert!(r.is_some());
    }

    #[test]
    fn regex_pass() {
        let r = Rule::Regex(r"^\d+$".to_string()).check("12345");
        assert!(r.is_none());
    }

    #[test]
    fn regex_fail() {
        let r = Rule::Regex(r"^\d+$".to_string()).check("abc");
        assert!(r.is_some());
    }

    #[test]
    fn not_regex_pass() {
        let r = Rule::NotRegex(r"\bfoo\b".to_string()).check("bar");
        assert!(r.is_none());
    }

    #[test]
    fn not_regex_fail() {
        let r = Rule::NotRegex(r"\bfoo\b".to_string()).check("contains foo here");
        assert!(r.is_some());
    }

    #[test]
    fn allowed_values_pass() {
        let r = Rule::AllowedValues(vec!["yes".to_string(), "no".to_string()]).check("yes");
        assert!(r.is_none());
    }

    #[test]
    fn allowed_values_fail() {
        let r = Rule::AllowedValues(vec!["yes".to_string(), "no".to_string()]).check("maybe");
        assert!(r.is_some());
    }

    #[test]
    fn no_pii_email_detected() {
        let r = Rule::NoPii.check("contact user@example.com for help");
        assert!(r.is_some());
    }

    #[test]
    fn no_pii_phone_detected() {
        let r = Rule::NoPii.check("call 555-867-5309 now");
        assert!(r.is_some());
    }

    #[test]
    fn no_pii_clean_text_passes() {
        let r = Rule::NoPii.check("The answer is 42");
        assert!(r.is_none());
    }

    #[test]
    fn valid_json_pass() {
        let r = Rule::ValidJson.check(r#"{"ok": true}"#);
        assert!(r.is_none());
    }

    #[test]
    fn valid_json_fail() {
        let r = Rule::ValidJson.check("not json");
        assert!(r.is_some());
    }

    #[test]
    fn non_empty_pass() {
        let r = Rule::NonEmpty.check("hello");
        assert!(r.is_none());
    }

    #[test]
    fn non_empty_fail_blank() {
        let r = Rule::NonEmpty.check("   ");
        assert!(r.is_some());
    }

    #[test]
    fn non_empty_fail_empty() {
        let r = Rule::NonEmpty.check("");
        assert!(r.is_some());
    }

    #[test]
    fn validator_all_pass() {
        let v = Validator::new(vec![Rule::MinLength(1), Rule::MaxLength(100)]);
        let r = v.validate("hello");
        assert!(r.ok);
        assert!(r.violations.is_empty());
    }

    #[test]
    fn validator_multiple_violations() {
        let v = Validator::new(vec![Rule::MinLength(100), Rule::ValidJson]);
        let r = v.validate("hi");
        assert!(!r.ok);
        assert_eq!(r.violations.len(), 2);
    }

    #[test]
    fn check_or_raise_ok() {
        let v = Validator::new(vec![Rule::NonEmpty]);
        assert!(v.check_or_raise("hello").is_ok());
    }

    #[test]
    fn check_or_raise_err() {
        let v = Validator::new(vec![Rule::NonEmpty]);
        let err = v.check_or_raise("").unwrap_err();
        assert!(!err.violations.is_empty());
    }

    #[test]
    fn validation_error_display() {
        let err = ValidationError {
            violations: vec!["NonEmpty: text is empty or whitespace".to_string()],
        };
        assert!(err.to_string().contains("ValidationError"));
    }

    #[test]
    fn empty_rules_always_ok() {
        let v = Validator::new(vec![]);
        assert!(v.validate("").ok);
    }

    #[test]
    fn valid_json_array_passes() {
        let r = Rule::ValidJson.check("[1, 2, 3]");
        assert!(r.is_none());
    }

    #[test]
    fn ssn_pattern_detected() {
        let r = Rule::NoPii.check("my ssn is 123-45-6789 please keep safe");
        assert!(r.is_some());
    }
}
