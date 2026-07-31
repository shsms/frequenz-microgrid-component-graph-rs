# Frequenz Component Graph Release Notes

## Summary

<!-- Here goes a general summary of what this release is about -->

## Upgrading

<!-- Here goes notes on how to upgrade from previous versions, including deprecations and what they should be replaced with -->

## New Features

<!-- Here goes the main new features and examples or instructions on how to use them -->

## Bug Fixes

- The consumer formula counted battery, PV, CHP, EV charger, wind turbine and steam boiler chains as site consumption when the grid connection point has a direct child that is not a grid meter — an inverter wired straight to the grid, for example. Those chains are now subtracted, as they already were when only grid meters sit below the grid connection point. A chain fed from outside the summed meters stays counted, because the sum never added its power in the first place. Graphs that set `include_phantom_loads_in_consumer_formula` are not affected.

- The consumer formula subtracted part of a battery, PV, CHP, EV charger, wind turbine or steam boiler chain twice when that chain is fed from two places — through a meter of its own and directly from a second meter. The meter above the chain reads only the part flowing through it, but its reading was subtracted as a whole chain's, on top of the chain's own reading. Site consumption came out too low, often clamped to zero. Such a chain is now subtracted once, through one term that covers both feeds. When the chain has no reading of its own to give, the meter's reading stands in for the one feed it carries. When even the meter reports nothing, nothing is subtracted for that chain. Graphs that set `include_phantom_loads_in_consumer_formula` are not affected.

- The consumer formula counted the power a battery, PV, CHP, EV charger, wind turbine or steam boiler chain draws through a silent meter as site consumption when the chain sits behind two parallel meters and one reports nothing: the silent feed's flow was never subtracted. Such a chain is now subtracted through its own reading, which covers both feeds. Graphs that set `include_phantom_loads_in_consumer_formula` are not affected.

- The consumer formula reported the site's battery, PV, CHP, EV charger, wind turbine and steam boiler chains, with the sign flipped, as consumption when the grid meter provides no telemetry. There is no grid reading to subtract them from in that case, so the formula is now `None`, as `grid_formula` already was. Graphs that set `include_phantom_loads_in_consumer_formula` are not affected.

- The consumer formula subtracted battery, PV, CHP, EV charger, wind turbine and steam boiler chains behind a grid meter that provides no telemetry, when another grid meter reports. A silent grid meter contributes nothing to the grid reading, so those chains were taken out of readings that never carried them — a discharging battery on the silent feed inflated site consumption. Chains behind a silent grid meter are no longer subtracted. Graphs that set `include_phantom_loads_in_consumer_formula` are not affected.
