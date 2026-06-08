# llm-output-validator

[![CI](https://github.com/MukundaKatta/llm-output-validator-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MukundaKatta/llm-output-validator-rs/actions/workflows/ci.yml)

A small, dependency-light, rule-based validator for LLM output strings.

Before you pass an LLM response downstream, run it through a set of composable
rules. Every rule runs (there is no short-circuit), so a single call gives you a
complete list of *all* the ways the output failed, not just the first one.

## Why

LLM outputs are unstructured and unreliable. A guardrail layer lets you assert
cheap, deterministic invariants ("must be valid JSON", "must be under 500
characters", "must not leak an email address") before the output reaches a
parser, a tool call, or a user. This crate gives you those invariants as plain
data (`Rule`) that you can build, store, and compose.

## Install

Add it to your `Cargo.toml`:

```toml
[dependencies]
llm-output-validator = "0.1"
```

## Usage

```rust
use llm_output_validator::{Rule, Validator};

// Compose a validator from a list of rules.
let validator = Validator::new(vec![
    Rule::NonEmpty,
    Rule::MinLength(5),
    Rule::MaxLength(500),
    Rule::ValidJson,
    Rule::NoPii,
]);

// A good response passes every rule.
let result = validator.validate(r#"{"answer": "The capital is Paris."}"#);
assert!(result.ok);
assert!(result.violations.is_empty());

// A bad response collects *all* violations, not just the first.
let bad = validator.validate("hi");
assert!(!bad.ok);
// "hi" is shorter than MinLength(5) and is not valid JSON.
assert_eq!(bad.violations.len(), 2);
```

### Failing fast with `check_or_raise`

When you would rather propagate an error than inspect a result, use
`check_or_raise`, which returns a `ValidationError` (implements
`std::error::Error`) listing every violation:

```rust
use llm_output_validator::{Rule, Validator};

let validator = Validator::new(vec![Rule::NonEmpty]);

match validator.check_or_raise("   ") {
    Ok(()) => println!("output is valid"),
    Err(e) => eprintln!("{e}"), // "ValidationError: NonEmpty: text is empty or whitespace"
}
```

### Checking a single rule

Each `Rule` can be checked on its own. `Rule::check` returns
`Option<String>`: `None` when the rule passes, or `Some(message)` describing the
violation:

```rust
use llm_output_validator::Rule;

assert_eq!(Rule::MinLength(3).check("hello"), None);
assert!(Rule::MinLength(10).check("hi").is_some());
```

## Built-in rules

| Rule | Passes when the text… |
| --- | --- |
| `MinLength(n)` | has at least `n` Unicode characters |
| `MaxLength(n)` | has at most `n` Unicode characters |
| `Regex(pattern)` | matches the regex `pattern` |
| `NotRegex(pattern)` | does **not** match the regex `pattern` |
| `AllowedValues(values)` | equals one of the exact strings in `values` |
| `NoPii` | contains no email, US-phone, or SSN-shaped pattern |
| `ValidJson` | parses as valid JSON (object, array, or scalar) |
| `NonEmpty` | is not empty or whitespace-only |

Lengths are measured in Unicode scalar values (`char`s), so multi-byte
characters count as one each.

> **Note:** `NoPii` uses simple, conservative regular expressions and is a
> best-effort heuristic, not a compliance-grade PII detector. Treat it as a
> tripwire, not a guarantee.

## API

- **`Rule`** — an enum of the validation rules listed above.
  - `Rule::check(&self, text: &str) -> Option<String>` — check one rule.
- **`Validator`** — a composed set of rules.
  - `Validator::new(rules: Vec<Rule>) -> Validator`
  - `Validator::validate(&self, text: &str) -> ValidationResult` — run every rule.
  - `Validator::check_or_raise(&self, text: &str) -> Result<(), ValidationError>`
- **`ValidationResult`** — `{ ok: bool, violations: Vec<String> }`.
- **`ValidationError`** — `{ violations: Vec<String> }`, implements
  `std::fmt::Display` and `std::error::Error`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
