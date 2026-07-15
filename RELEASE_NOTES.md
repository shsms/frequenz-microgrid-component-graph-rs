# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

- The `AggregationFormula` and `CoalesceFormula` types and the `Formula` trait are replaced by a single `Formula` struct. The `*_formula` methods all return `Formula` now; combine formulas with `+`, `-`, and the `Formula::coalesce` / `min` / `max` methods.

- `prefer_meters_in_component_formulas` now defaults to `false`: the per-category formulas use the component readings as the primary source and the meter as the fallback. Set it to `true` to restore the old meter-first order. This changes only the order of the sources; the new meter-subtraction fallback terms are used either way.

- Graph validation failures are now reported as `ErrorKind::ValidationErrors(Vec<ValidationError>)` rather than flattened into a single `InvalidGraph` error. Their `Display` changed accordingly: each failure is listed on its own line under a `Graph validation failed:` header, without the old per-line `InvalidGraph:` prefixes.

## New Features

- `ErrorKind` and `ValidationError` are now public. `Error::kind()` exposes the kind, and each `ValidationError` reports its `message()` and the `component_ids()` it involves, so individual validation failures (including detected cycles) can be inspected programmatically instead of parsed from a string.

- Components that share a meter with sibling meters or components of another category (e.g. PV inverters next to a battery sub-meter under one "PV + battery" meter) now fall back to the parent meter minus those siblings (`COALESCE(..., #parent - #sub, ...)`) when their own readings are missing. This also applies to partial groups, so a single unreachable inverter can be measured as the meter minus its working siblings.

- The consumer formula now measures the non-consumer components behind one internal meter as one group: it subtracts `COALESCE(#meter, device readings...)` instead of each device on its own. The meter reading is used when it is available, and a shared meter is never subtracted twice. Note: if such a meter also carries a load that is not in the component graph, that load is now subtracted together with the group.

## Bug Fixes

- Fixed double-counting of a component fed by multiple parallel meters (a diamond topology): it is now measured as a single diamond term instead of once per parent meter.
