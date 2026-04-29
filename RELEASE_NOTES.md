# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

- `predecessors` / `successors` now walk past pass-through (untracked) component categories (`Converter`, `Breaker`, `Precharger`, `Electrolyzer`, `PowerTransformer`, `Hvac`, `Plc`, `CryptoMiner`, `StaticTransferSwitch`, `UninterruptiblePowerSupply`, `CapacitorBank`), making them transparent to formula generation and neighbor-rule validation. Use the new `raw_predecessors` / `raw_successors` if you need the literal graph-direct view that includes them.

## New Features

A new component category, steam boiler, was added.

## Bug Fixes

<!-- Here goes notable bug fixes that are worth a special mention or explanation -->
