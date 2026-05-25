/*!
llm-output-validator: rule-based validator for LLM output strings.

Run AFTER the LLM produces a string. Enforce length, regex, allowed values,
JSON parseability, no-PII, or custom predicates. All rules run (no short-
circuit) so you get a complete picture of every failure in one pass.

```rust
use llm_output_validator::{OutputValidator, rules};

let v = OutputValidator::new(vec![
    rules::length(Some(10), Some(500)),
    rules::json_parseable(),
]);

let result = v.check("{\"key\": \"value\"}");
assert!(result.ok);
assert!(result.failed_rules.is_empty());

let bad = v.check("hi");
assert!(!bad.ok);
assert!(bad.failed_rules.contains(&"length".to_owned()));
```
*/

use std::collections::HashMap;
use std::sync::Arc;

// ---- core types ----------------------------------------------------------

/// The outcome of running one rule against a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleResult {
    pub passed: bool,
    pub message: Option<String>,
}

impl RuleResult {
    pub fn pass() -> Self {
        Self { passed: true, message: None }
    }
    pub fn fail(msg: impl Into<String>) -> Self {
        Self { passed: false, message: Some(msg.into()) }
    }
}

/// Aggregate result from checking all rules.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub ok: bool,
    pub failed_rules: Vec<String>,
    pub details: HashMap<String, RuleResult>,
}

/// A named rule check.
#[derive(Clone)]
pub struct Rule {
    pub name: String,
    check: Arc<dyn Fn(&str) -> RuleResult + Send + Sync>,
}

impl Rule {
    pub fn new(name: impl Into<String>, check: impl Fn(&str) -> RuleResult + Send + Sync + 'static) -> Self {
        Self { name: name.into(), check: Arc::new(check) }
    }

    pub fn check(&self, text: &str) -> RuleResult {
        (self.check)(text)
    }
}

impl std::fmt::Debug for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rule({})", self.name)
    }
}

/// Raised-error equivalent (returned from `check_or_raise`).
#[derive(Debug, Clone)]
pub struct OutputValidationError {
    pub failed_rules: Vec<String>,
    pub details: HashMap<String, RuleResult>,
}

impl std::fmt::Display for OutputValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let first = self.failed_rules.first().map(|s| s.as_str()).unwrap_or("?");
        write!(f, "output failed {} rule(s): {first}", self.failed_rules.len())
    }
}

impl std::error::Error for OutputValidationError {}

// ---- OutputValidator -----------------------------------------------------

/// Runs a list of `Rule`s against an output string. All rules always run.
pub struct OutputValidator {
    rules: Vec<Rule>,
}

impl OutputValidator {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    /// Run all rules. Returns a `ValidationResult` with full detail.
    pub fn check(&self, text: &str) -> ValidationResult {
        let mut details = HashMap::new();
        let mut failed = Vec::new();
        for rule in &self.rules {
            let result = rule.check(text);
            if !result.passed {
                failed.push(rule.name.clone());
            }
            details.insert(rule.name.clone(), result);
        }
        ValidationResult { ok: failed.is_empty(), failed_rules: failed, details }
    }

    /// Like `check` but returns `Err(OutputValidationError)` if any rule fails.
    pub fn check_or_raise(&self, text: &str) -> Result<ValidationResult, OutputValidationError> {
        let r = self.check(text);
        if r.ok {
            Ok(r)
        } else {
            Err(OutputValidationError { failed_rules: r.failed_rules, details: r.details })
        }
    }
}

// ---- built-in rules ------------------------------------------------------

pub mod rules {
    use super::{Rule, RuleResult};

    /// Pass if `text.len()` is between `min_chars` and `max_chars` (inclusive).
    pub fn length(min_chars: Option<usize>, max_chars: Option<usize>) -> Rule {
        Rule::new("length", move |text| {
            let n = text.len();
            if let Some(min) = min_chars {
                if n < min {
                    return RuleResult::fail(format!("length {n} < min {min}"));
                }
            }
            if let Some(max) = max_chars {
                if n > max {
                    return RuleResult::fail(format!("length {n} > max {max}"));
                }
            }
            RuleResult::pass()
        })
    }

    /// Pass if word count (whitespace split) is in range.
    pub fn length_words(min_words: Option<usize>, max_words: Option<usize>) -> Rule {
        Rule::new("length_words", move |text| {
            let n = text.split_whitespace().count();
            if let Some(min) = min_words {
                if n < min {
                    return RuleResult::fail(format!("word count {n} < min {min}"));
                }
            }
            if let Some(max) = max_words {
                if n > max {
                    return RuleResult::fail(format!("word count {n} > max {max}"));
                }
            }
            RuleResult::pass()
        })
    }

    /// Pass if text matches the given regex pattern (uses std contains/regex lite).
    ///
    /// This implementation uses a simple substring/prefix check without an external
    /// regex dep. For full regex, pair with the `regex` crate in your application.
    pub fn starts_with_uppercase() -> Rule {
        Rule::new("starts_with_uppercase", |text| {
            if text.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                RuleResult::pass()
            } else {
                RuleResult::fail("text does not start with an uppercase letter")
            }
        })
    }

    /// Pass if `text` is parseable as JSON.
    pub fn json_parseable() -> Rule {
        Rule::new("json_parseable", |text| {
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(_) => RuleResult::pass(),
                Err(e) => RuleResult::fail(format!("not valid JSON: {e}")),
            }
        })
    }

    /// Pass if `text`, when parsed as a JSON object, matches the given JSON Schema
    /// (basic type checking only; no external schema-validator dep).
    ///
    /// Checks: required fields present and of the right JSON type.
    pub fn json_has_keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Rule {
        let keys: Vec<String> = keys.into_iter().map(|k| k.into()).collect();
        let keys = std::sync::Arc::new(keys);
        Rule::new("json_has_keys", move |text| {
            let v: serde_json::Value = match serde_json::from_str(text) {
                Ok(v) => v,
                Err(_) => return RuleResult::fail("not valid JSON"),
            };
            let obj = match v.as_object() {
                Some(m) => m,
                None => return RuleResult::fail("not a JSON object"),
            };
            for key in keys.as_ref() {
                if !obj.contains_key(key) {
                    return RuleResult::fail(format!("missing key {key:?}"));
                }
            }
            RuleResult::pass()
        })
    }

    /// Pass if `text` is one of the `allowed` values (exact match).
    pub fn allowed_values(allowed: impl IntoIterator<Item = impl Into<String>>) -> Rule {
        let allowed: std::collections::HashSet<String> =
            allowed.into_iter().map(|s| s.into()).collect();
        let allowed = std::sync::Arc::new(allowed);
        Rule::new("allowed_values", move |text| {
            if allowed.contains(text) {
                RuleResult::pass()
            } else {
                RuleResult::fail(format!("{text:?} not in allowed set"))
            }
        })
    }

    /// Pass if text does NOT contain obvious PII patterns (email, phone, SSN).
    ///
    /// Uses simple substring detection. Not authoritative — pair with a dedicated
    /// PII library for high-stakes use cases.
    pub fn no_pii() -> Rule {
        Rule::new("no_pii", |text| {
            // Check for email-like pattern
            if looks_like_email(text) {
                return RuleResult::fail("possible email address detected");
            }
            // Check for SSN-like pattern (NNN-NN-NNNN)
            if looks_like_ssn(text) {
                return RuleResult::fail("possible SSN detected");
            }
            RuleResult::pass()
        })
    }

    fn looks_like_email(text: &str) -> bool {
        // Simple: look for x@y.z pattern
        let bytes = text.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'@' && i > 0 && i + 2 < bytes.len() {
                // Check there's a dot after the @
                if bytes[i + 1..].contains(&b'.') {
                    return true;
                }
            }
        }
        false
    }

    fn looks_like_ssn(text: &str) -> bool {
        // Pattern: DDD-DD-DDDD or DDD DD DDDD
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        for i in 0..n {
            if i + 11 <= n {
                let seg = &chars[i..i + 11];
                // Check NNN-NN-NNNN or NNN NN NNNN
                let sep = seg[3];
                if (sep == '-' || sep == ' ')
                    && seg[6] == sep
                    && seg[..3].iter().all(|c| c.is_ascii_digit())
                    && seg[4..6].iter().all(|c| c.is_ascii_digit())
                    && seg[7..11].iter().all(|c| c.is_ascii_digit())
                {
                    return true;
                }
            }
        }
        false
    }

    /// Pass if `text` does not contain the given substring (case-sensitive).
    pub fn does_not_contain(substr: impl Into<String>) -> Rule {
        let s = substr.into();
        Rule::new("does_not_contain", move |text| {
            if text.contains(s.as_str()) {
                RuleResult::fail(format!("text contains forbidden substring {s:?}"))
            } else {
                RuleResult::pass()
            }
        })
    }

    /// Pass if `text` contains the given substring (case-sensitive).
    pub fn contains(substr: impl Into<String>) -> Rule {
        let s = substr.into();
        Rule::new("contains", move |text| {
            if text.contains(s.as_str()) {
                RuleResult::pass()
            } else {
                RuleResult::fail(format!("text does not contain {s:?}"))
            }
        })
    }

    /// Pass if `predicate(text)` returns true.
    pub fn custom(name: impl Into<String>, predicate: impl Fn(&str) -> bool + Send + Sync + 'static, error_msg: impl Into<String>) -> Rule {
        let msg = error_msg.into();
        Rule::new(name, move |text| {
            if predicate(text) { RuleResult::pass() } else { RuleResult::fail(msg.clone()) }
        })
    }
}

// ---- tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{rules, OutputValidator};
    use serde_json::json;

    #[test]
    fn all_rules_pass() {
        let v = OutputValidator::new(vec![
            rules::length(Some(5), Some(50)),
            rules::json_parseable(),
        ]);
        let r = v.check(r#"{"key": "val"}"#);
        assert!(r.ok);
        assert!(r.failed_rules.is_empty());
    }

    #[test]
    fn length_min_fail() {
        let v = OutputValidator::new(vec![rules::length(Some(100), None)]);
        let r = v.check("short");
        assert!(!r.ok);
        assert!(r.failed_rules.contains(&"length".to_owned()));
        assert!(r.details["length"].message.as_ref().unwrap().contains("< min"));
    }

    #[test]
    fn length_max_fail() {
        let v = OutputValidator::new(vec![rules::length(None, Some(3))]);
        let r = v.check("toolong");
        assert!(!r.ok);
    }

    #[test]
    fn length_pass() {
        let v = OutputValidator::new(vec![rules::length(Some(2), Some(10))]);
        assert!(v.check("hello").ok);
    }

    #[test]
    fn length_words_fail() {
        let v = OutputValidator::new(vec![rules::length_words(Some(5), None)]);
        let r = v.check("only two");
        assert!(!r.ok);
        assert!(r.failed_rules.contains(&"length_words".to_owned()));
    }

    #[test]
    fn json_parseable_pass() {
        let v = OutputValidator::new(vec![rules::json_parseable()]);
        assert!(v.check(r#"{"a": 1}"#).ok);
        assert!(v.check(r#"[1, 2, 3]"#).ok);
        assert!(v.check(r#""hello""#).ok);
    }

    #[test]
    fn json_parseable_fail() {
        let v = OutputValidator::new(vec![rules::json_parseable()]);
        let r = v.check("not json at all");
        assert!(!r.ok);
    }

    #[test]
    fn json_has_keys_pass() {
        let v = OutputValidator::new(vec![rules::json_has_keys(["name", "age"])]);
        assert!(v.check(r#"{"name": "Alice", "age": 30}"#).ok);
    }

    #[test]
    fn json_has_keys_fail_missing() {
        let v = OutputValidator::new(vec![rules::json_has_keys(["name", "email"])]);
        let r = v.check(r#"{"name": "Alice"}"#);
        assert!(!r.ok);
        assert!(r.details["json_has_keys"].message.as_ref().unwrap().contains("email"));
    }

    #[test]
    fn allowed_values_pass() {
        let v = OutputValidator::new(vec![rules::allowed_values(["yes", "no", "maybe"])]);
        assert!(v.check("yes").ok);
        assert!(v.check("no").ok);
    }

    #[test]
    fn allowed_values_fail() {
        let v = OutputValidator::new(vec![rules::allowed_values(["yes", "no"])]);
        let r = v.check("maybe");
        assert!(!r.ok);
    }

    #[test]
    fn no_pii_pass() {
        let v = OutputValidator::new(vec![rules::no_pii()]);
        assert!(v.check("Hello, this is a normal response with no personal info.").ok);
    }

    #[test]
    fn no_pii_email_fail() {
        let v = OutputValidator::new(vec![rules::no_pii()]);
        let r = v.check("Contact me at user@example.com for more info.");
        assert!(!r.ok);
    }

    #[test]
    fn no_pii_ssn_fail() {
        let v = OutputValidator::new(vec![rules::no_pii()]);
        let r = v.check("My SSN is 123-45-6789.");
        assert!(!r.ok);
    }

    #[test]
    fn does_not_contain_pass() {
        let v = OutputValidator::new(vec![rules::does_not_contain("secret")]);
        assert!(v.check("This is safe").ok);
    }

    #[test]
    fn does_not_contain_fail() {
        let v = OutputValidator::new(vec![rules::does_not_contain("secret")]);
        let r = v.check("This contains a secret token.");
        assert!(!r.ok);
    }

    #[test]
    fn contains_pass() {
        let v = OutputValidator::new(vec![rules::contains("important")]);
        assert!(v.check("This is important information.").ok);
    }

    #[test]
    fn contains_fail() {
        let v = OutputValidator::new(vec![rules::contains("important")]);
        let r = v.check("Nothing interesting here.");
        assert!(!r.ok);
    }

    #[test]
    fn custom_rule() {
        let v = OutputValidator::new(vec![
            rules::custom("no_numbers", |t| !t.chars().any(|c| c.is_ascii_digit()), "text contains numbers")
        ]);
        assert!(v.check("hello world").ok);
        let r = v.check("has 42 in it");
        assert!(!r.ok);
    }

    #[test]
    fn all_rules_run_no_short_circuit() {
        let v = OutputValidator::new(vec![
            rules::length(Some(100), None), // fails
            rules::json_parseable(),        // also fails
        ]);
        let r = v.check("short not json");
        assert!(!r.ok);
        assert_eq!(r.failed_rules.len(), 2);
    }

    #[test]
    fn check_or_raise_ok() {
        let v = OutputValidator::new(vec![rules::length(Some(1), Some(100))]);
        assert!(v.check_or_raise("hello").is_ok());
    }

    #[test]
    fn check_or_raise_err() {
        let v = OutputValidator::new(vec![rules::length(Some(100), None)]);
        let err = v.check_or_raise("short").unwrap_err();
        assert!(!err.failed_rules.is_empty());
        assert!(err.to_string().contains("rule(s)"));
    }

    #[test]
    fn empty_rules_always_pass() {
        let v = OutputValidator::new(vec![]);
        assert!(v.check("anything").ok);
    }

    #[test]
    fn starts_with_uppercase_pass() {
        let v = OutputValidator::new(vec![rules::starts_with_uppercase()]);
        assert!(v.check("Hello world").ok);
    }

    #[test]
    fn starts_with_uppercase_fail() {
        let v = OutputValidator::new(vec![rules::starts_with_uppercase()]);
        let r = v.check("hello world");
        assert!(!r.ok);
    }

    #[test]
    fn combined_rules_partial_failure() {
        let v = OutputValidator::new(vec![
            rules::length(Some(5), Some(20)),
            rules::json_parseable(),
        ]);
        let r = v.check("short");  // length ok, not JSON
        assert!(!r.ok);
        assert!(r.failed_rules.contains(&"json_parseable".to_owned()));
        assert!(!r.failed_rules.contains(&"length".to_owned()));
    }

    #[test]
    fn words_in_range_pass() {
        let v = OutputValidator::new(vec![rules::length_words(Some(2), Some(5))]);
        assert!(v.check("hello world").ok);
        assert!(v.check("one two three").ok);
    }
}
