# Frequenz Component Graph Release Notes

## Upgrading

- The PV, battery, CHP, EV and Wind Turbine formulas now prefer meters as the primary components and fallback to inverters or other corresponding components only when the meters are not available.

  This makes a big difference in performance when there are multiple PV inverters behind a single meter, for example.

  This behaviour can be changed with the newly introduced meter preference config flags.

- Consumer formulas don't consider phantom loads by default anymore.  The original behaviour is still available through a config flag.

## New Features

- This introduces a new consumer formula generator that doesn't consider phantom loads.

  Meters with successors can still have loads not represented in the component graph.  These are called phantom loads.
