# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

<!-- Here goes notes on how to upgrade from previous versions, including deprecations and what they should be replaced with -->

## New Features

- The `Node` trait has a new `operational_mode()` method. The default is `OperationalMode::Unspecified`, which is treated as providing telemetry, so existing `Node` implementations keep their behavior. A component whose mode provides no telemetry is not used as a measurement source in formulas. It is still used to classify the meter that measures it (e.g. as a PV meter or a CHP meter). A coalesce formula can be `None` when no source component provides telemetry.

## Bug Fixes

<!-- Here goes notable bug fixes that are worth a special mention or explanation -->
