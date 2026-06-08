//! Integration tests exercising the public API of `llm-output-validator`
//! exactly as a downstream consumer would use it (no access to private items).

use llm_output_validator::{Rule, ValidationError, Validator};

#[test]
fn composed_validator_passes_clean_json() {
    let v = Validator::new(vec![
        Rule::NonEmpty,
        Rule::MinLength(5),
        Rule::MaxLength(500),
        Rule::ValidJson,
        Rule::NoPii,
    ]);

    let result = v.validate(r#"{"answer": "The capital is Paris."}"#);
    assert!(result.ok);
    assert!(result.violations.is_empty());
}

#[test]
fn all_rules_run_no_short_circuit() {
    // "hi" is non-empty, too short for MinLength(5), within MaxLength,
    // not valid JSON, and contains no PII -> exactly two violations.
    let v = Validator::new(vec![
        Rule::NonEmpty,
        Rule::MinLength(5),
        Rule::MaxLength(500),
        Rule::ValidJson,
        Rule::NoPii,
    ]);

    let result = v.validate("hi");
    assert!(!result.ok);
    assert_eq!(result.violations.len(), 2);
    // Violation messages are human-readable and name the rule.
    assert!(result.violations.iter().any(|m| m.starts_with("MinLength")));
    assert!(result.violations.iter().any(|m| m.starts_with("ValidJson")));
}

#[test]
fn check_or_raise_surfaces_every_violation() {
    let v = Validator::new(vec![Rule::NonEmpty, Rule::ValidJson]);

    let err: ValidationError = v.check_or_raise("   ").unwrap_err();
    // Whitespace-only is empty (NonEmpty) and not JSON (ValidJson).
    assert_eq!(err.violations.len(), 2);

    let display = err.to_string();
    assert!(display.contains("ValidationError"));
    assert!(display.contains("NonEmpty"));
    assert!(display.contains("ValidJson"));
}

#[test]
fn check_or_raise_ok_on_valid_input() {
    let v = Validator::new(vec![Rule::NonEmpty, Rule::MinLength(1)]);
    assert!(v.check_or_raise("hello").is_ok());
}

#[test]
fn length_is_measured_in_unicode_chars_not_bytes() {
    // "café" is 4 chars but 5 bytes; "🎉" is 1 char but 4 bytes.
    let v = Validator::new(vec![Rule::MinLength(4), Rule::MaxLength(4)]);
    assert!(v.validate("café").ok);

    let single_emoji = Validator::new(vec![Rule::MinLength(1), Rule::MaxLength(1)]);
    assert!(single_emoji.validate("🎉").ok);
}

#[test]
fn allowed_values_is_exact_match() {
    let v = Validator::new(vec![Rule::AllowedValues(vec![
        "yes".to_string(),
        "no".to_string(),
    ])]);

    assert!(v.validate("yes").ok);
    assert!(!v.validate("YES").ok); // case-sensitive
    assert!(!v.validate("maybe").ok);
}

#[test]
fn regex_and_not_regex_complement_each_other() {
    let digits_only = Validator::new(vec![Rule::Regex(r"^\d+$".to_string())]);
    assert!(digits_only.validate("12345").ok);
    assert!(!digits_only.validate("12a45").ok);

    let no_profanity = Validator::new(vec![Rule::NotRegex(r"(?i)\bbadword\b".to_string())]);
    assert!(no_profanity.validate("a clean sentence").ok);
    assert!(!no_profanity.validate("this has a BadWord in it").ok);
}

#[test]
fn invalid_regex_pattern_is_reported_as_violation() {
    // An unbalanced bracket is not a valid regex; rather than panicking, the
    // rule reports a violation so a bad pattern can never let bad output pass.
    let v = Validator::new(vec![Rule::Regex("[".to_string())]);
    let result = v.validate("anything");
    assert!(!result.ok);
    assert_eq!(result.violations.len(), 1);
    assert!(result.violations[0].contains("invalid pattern"));
}

#[test]
fn no_pii_detects_common_patterns() {
    let v = Validator::new(vec![Rule::NoPii]);
    assert!(!v.validate("reach me at user@example.com").ok);
    assert!(!v.validate("call 555-867-5309").ok);
    assert!(!v.validate("ssn 123-45-6789").ok);
    assert!(v.validate("The answer is 42.").ok);
}

#[test]
fn empty_validator_accepts_everything() {
    let v = Validator::new(vec![]);
    assert!(v.validate("").ok);
    assert!(v.validate("literally anything").ok);
}

#[test]
fn valid_json_accepts_objects_arrays_and_scalars() {
    let v = Validator::new(vec![Rule::ValidJson]);
    assert!(v.validate(r#"{"k": 1}"#).ok);
    assert!(v.validate("[1, 2, 3]").ok);
    assert!(v.validate("42").ok);
    assert!(v.validate("\"a string\"").ok);
    assert!(!v.validate("{not json}").ok);
}
