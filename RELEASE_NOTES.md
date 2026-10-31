# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

<!-- Here goes notes on how to upgrade from previous versions, including deprecations and what they should be replaced with -->

## New Features

<!-- Here goes the main new features and examples or instructions on how to use them -->

## Bug Fixes

- `ComponentGraph::try_new` no longer fails on a component of category `Unspecified` that has no connections. It logs a warning for the component, and `components()` and `component()` can return it. Aggregate and per-category formulas leave the component out, even if it is a real device. A component of category `Unspecified` that has a connection still makes `try_new` fail.
