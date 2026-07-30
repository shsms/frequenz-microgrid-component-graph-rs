# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

<!-- Here goes notes on how to upgrade from previous versions, including deprecations and what they should be replaced with -->

## New Features

<!-- Here goes the main new features and examples or instructions on how to use them -->

## Bug Fixes

- The consumer formula subtracted part of a battery, PV, CHP, EV charger, wind turbine or steam boiler chain twice when that chain is fed from two places — through a meter of its own and directly from a second meter. The meter above the chain reads only the part flowing through it, but its reading was subtracted as a whole chain's, on top of the chain's own reading. Site consumption came out too low, often clamped to zero. Such a chain is now subtracted once, through one term that covers both feeds. When the chain has no reading of its own to give, the meter's reading stands in for the one feed it carries. When even the meter reports nothing, nothing is subtracted for that chain. Graphs that set `include_phantom_loads_in_consumer_formula` are not affected.
