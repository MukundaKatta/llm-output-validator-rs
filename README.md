# llm-output-validator

Rule-based validator for LLM output strings. All rules run (no short-circuit) so you get a complete picture of every failure.

## Usage

```rust
use llm_output_validator::{OutputValidator, rules};

let v = OutputValidator::new(vec![
    rules::length(Some(10), Some(500)),
    rules::json_parseable(),
    rules::no_pii(),
]);

let result = v.check(r#"{"answer": "The capital is Paris."}"#);
assert!(result.ok);

let bad = v.check("hi");
assert!(!bad.ok);
assert!(bad.failed_rules.contains(&"length".to_owned()));
```

## Built-in rules

- `length(min, max)` — character count
- `length_words(min, max)` — word count
- `json_parseable()` — valid JSON
- `json_has_keys([...])` — required object keys present
- `allowed_values([...])` — exact match against a set
- `no_pii()` — no email or SSN-like patterns
- `contains(substr)` / `does_not_contain(substr)` — substring checks
- `starts_with_uppercase()` — capitalization check
- `custom(name, predicate, msg)` — bring your own

## License

MIT OR Apache-2.0
