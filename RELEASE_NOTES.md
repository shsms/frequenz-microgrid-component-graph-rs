# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

- The `AggregationFormula` and `CoalesceFormula` types and the `Formula` trait are replaced by a single `Formula` struct. The `*_formula` methods all return `Formula` now; combine formulas with `+`, `-`, and the `Formula::coalesce` / `min` / `max` methods.

- Graph validation failures are now reported as `ErrorKind::ValidationErrors(Vec<ValidationError>)` rather than flattened into a single `InvalidGraph` error. Their `Display` changed accordingly: each failure is listed on its own line under a `Graph validation failed:` header, without the old per-line `InvalidGraph:` prefixes.

## New Features

- `ErrorKind` and `ValidationError` are now public. `Error::kind()` exposes the kind, and each `ValidationError` reports its `message()` and the `component_ids()` it involves, so individual validation failures (including detected cycles) can be inspected programmatically instead of parsed from a string.

## Bug Fixes

<!-- Here goes notable bug fixes that are worth a special mention or explanation -->
