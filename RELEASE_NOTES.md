# Frequenz Component Graph Release Notes

## Upgrading

- `predecessors` / `successors` now walk past pass-through (untracked) component categories (`Converter`, `Breaker`, `Precharger`, `Electrolyzer`, `PowerTransformer`, `Hvac`, `Plc`, `CryptoMiner`, `StaticTransferSwitch`, `UninterruptiblePowerSupply`, `CapacitorBank`), making them transparent to formula generation and neighbor-rule validation. Use the new `raw_predecessors` / `raw_successors` if you need the literal graph-direct view that includes them.

- Use the builder pattern from now on to construct `ComponentGraphConfig`: fields are `pub(crate)` and construction goes through the new `ComponentGraphConfig::builder()`. With this, adding a new config option will no longer be a breaking change. The six per-category `prefer_X_in_Y_formula` flags are replaced by a global `prefer_meters_in_component_formulas` with per-formula overrides via `FormulaOverrides`.

## New Features

A new component category, steam boiler, was added.
