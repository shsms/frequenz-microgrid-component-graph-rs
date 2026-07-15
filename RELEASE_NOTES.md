# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

- The `AggregationFormula` and `CoalesceFormula` types and the `Formula` trait are replaced by a single `Formula` struct. The `*_formula` methods all return `Formula` now; combine formulas with `+`, `-`, and the `Formula::coalesce` / `min` / `max` methods.

- Graph validation failures are now reported as `ErrorKind::ValidationErrors(Vec<ValidationError>)` rather than flattened into a single `InvalidGraph` error. Their `Display` changed accordingly: each failure is listed on its own line under a `Graph validation failed:` header, without the old per-line `InvalidGraph:` prefixes.

## New Features

- `ErrorKind` and `ValidationError` are now public. `Error::kind()` exposes the kind, and each `ValidationError` reports its `message()` and the `component_ids()` it involves, so individual validation failures (including detected cycles) can be inspected programmatically instead of parsed from a string.

- Components that share a meter with sibling meters (e.g. PV inverters next to a battery sub-meter under one "PV + battery" meter) are now measured as the parent meter minus the sibling meters, with the component readings as the fallback: `COALESCE(#parent - #sub, ...)`.

## Bug Fixes

- Fixed double-counting of a component fed by multiple parallel meters (a diamond topology): it is now measured as a single diamond term instead of once per parent meter.
