# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

- `component_formula()` and `component_ac_coalesce_formula()` now check the given component id and return an error when it is not in the graph. Before, they returned a formula for any id. Handle the error, or pass only ids that are in the graph.

## New Features

- The `Node` trait has a new `operational_mode()` method. The default is `OperationalMode::Unspecified`, which is treated as providing telemetry, so existing `Node` implementations keep their behavior. A component whose mode provides no telemetry is not used as a measurement source in formulas. It is still used to classify the meter that measures it (e.g. as a PV meter or a CHP meter). A coalesce formula can be `None` when no source component provides telemetry.

- For a component that provides no telemetry, `component_formula()` and `component_ac_coalesce_formula()` both return `None`. This holds for a meter too.

## Bug Fixes

<!-- Here goes notable bug fixes that are worth a special mention or explanation -->
