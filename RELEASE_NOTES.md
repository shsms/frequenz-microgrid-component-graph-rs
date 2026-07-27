# Frequenz Component Graph Release Notes

## Summary

This release lets formulas take a component's operational mode into account. The new `OperationalMode` enum and the `Node::operational_mode()` method tell the graph which components report telemetry. A component that provides no telemetry is not used as a measurement source, but it still classifies the meter that measures it.

## New Features

- The `Node` trait has a new `operational_mode()` method. The default is `OperationalMode::Unspecified`, and a component in that mode is treated as providing telemetry, so existing `Node` implementations keep their behavior.

- A component that provides no telemetry is not used as a measurement source in formulas. It is still used to classify the meter that measures it (e.g. as a PV meter or a CHP meter). What the formulas do instead:

  - a component can still be measured through the meter above it;
  - a meter is measured through its children, since it has no reading of its own — except a grid meter, which can carry loads that are not in the component graph, so the formula gets `None` for it;
  - the consumer formula descends past such a meter and sums its reporting descendant meters;
  - `component_formula()` and `component_ac_coalesce_formula()` return `None`, for a meter as well as for any other component.

  A formula can also lose all its measurement sources. `pv_formula()` is `0.0` when the only PV inverter provides no telemetry and no meter measures it. `grid_formula()` is `None` when the grid meter provides no telemetry. A coalesce formula is `None` when no source component provides telemetry.

## Bug Fixes

- `component_formula()` and `component_ac_coalesce_formula()` returned a formula for any component id. They now return an error when the given id is not in the graph.
