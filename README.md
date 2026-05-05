# Frequenz Microgrid Component Graph

[<img alt="docs.rs" src="https://img.shields.io/docsrs/frequenz-microgrid-component-graph">](https://docs.rs/frequenz-microgrid-component-graph)
[<img alt="Crates.io" src="https://img.shields.io/crates/v/frequenz-microgrid-component-graph">](https://crates.io/crates/frequenz-microgrid-component-graph)

A Rust library for modelling a microgrid as a Directed Acyclic Graph
(DAG) of electrical components, validating the topology, and generating
string formulas for aggregated metrics.

## Usage

The central type is [`ComponentGraph`], generic over user-supplied node
and edge types. Build one by passing iterators of components and
connections to [`ComponentGraph::try_new`], optionally with a
[`ComponentGraphConfig`] constructed via
[`ComponentGraphConfig::builder()`][builder].

## The `Node` and `Edge` traits

Because this crate is independent of any specific microgrid representation,
it doesn't know about the component and connection types and instead uses
traits to interact with them.

To be usable with [`ComponentGraph`], the component and connection types
must therefore implement the [`Node`] and [`Edge`] traits, respectively.
Check the rustdoc for these traits for sample implementations.

## Validation

[`try_new`][`ComponentGraph::try_new`] returns an [`Error`] unless the
input graph has exactly one root (a [`GridConnectionPoint`]), is fully
connected, acyclic, and obeys the per-category neighbor rules (e.g. a
battery has a battery inverter predecessor and no successors). Selected
checks downgrade to a `tracing::warn!` when the corresponding `allow_*`
flag is set on the config.

## Pass-through components

Several categories (transformers, breakers, converters, electrolyzers,
HVAC, capacitor banks, and similar non-metering interconnect components)
are treated as *pass-through*: validators and formula generators see
the graph as if those nodes weren't there, and the public
[`predecessors`] / [`successors`] iterators walk past them transparently.
The raw graph view, including pass-through nodes, is available via
[`raw_predecessors`] / [`raw_successors`].
[`ComponentGraph::try_new`] emits a `tracing::warn!` for each
pass-through node so operators know what is being elided.

> **Warning:** Pass-through behavior exists for forward compatibility —
> when new component categories are introduced, the library treats it
> as transparent rather than failing validation of the whole graph.
>
> The trade-off is that a real misclassification can slip through
> unnoticed if the `tracing::warn!` output isn't being read; operators
> integrating this library should make sure those warnings are
> surfaced.

## Formulas

Once constructed, the graph emits string formulas (e.g.
`#1 + COALESCE(#2, #3, 0.0)`) that downstream code can hand to a metric
evaluator:

- Aggregate: [`grid_formula`], [`consumer_formula`], [`producer_formula`].
- Per category: [`battery_formula`], [`pv_formula`], [`chp_formula`],
  [`wind_turbine_formula`], [`ev_charger_formula`], [`steam_boiler_formula`].
- Single component: [`component_formula`].
- Non-aggregating metrics (voltage, frequency): [`grid_coalesce_formula`],
  [`battery_ac_coalesce_formula`], [`pv_ac_coalesce_formula`],
  [`component_ac_coalesce_formula`].

For the per-category formulas, whether the meter or the device
measurement is the primary source is controlled by
[`prefer_meters_in_component_formulas`] on the config, with per-formula
overrides available through [`FormulaOverrides`].

[`ComponentGraph`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html
[`ComponentGraphConfig`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraphConfig.html
[`Node`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/trait.Node.html
[`Edge`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/trait.Edge.html
[`Error`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.Error.html
[`FormulaOverrides`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.FormulaOverrides.html
[`GridConnectionPoint`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/enum.ComponentCategory.html#variant.GridConnectionPoint
[`ComponentGraph::try_new`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.try_new
[builder]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraphConfig.html#method.builder
[`predecessors`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.predecessors
[`successors`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.successors
[`raw_predecessors`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.raw_predecessors
[`raw_successors`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.raw_successors
[`grid_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.grid_formula
[`consumer_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.consumer_formula
[`producer_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.producer_formula
[`battery_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.battery_formula
[`pv_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.pv_formula
[`chp_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.chp_formula
[`wind_turbine_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.wind_turbine_formula
[`ev_charger_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.ev_charger_formula
[`steam_boiler_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.steam_boiler_formula
[`component_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.component_formula
[`grid_coalesce_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.grid_coalesce_formula
[`battery_ac_coalesce_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.battery_ac_coalesce_formula
[`pv_ac_coalesce_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.pv_ac_coalesce_formula
[`component_ac_coalesce_formula`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraph.html#method.component_ac_coalesce_formula
[`prefer_meters_in_component_formulas`]: https://docs.rs/frequenz-microgrid-component-graph/latest/frequenz_microgrid_component_graph/struct.ComponentGraphConfigBuilder.html#method.prefer_meters_in_component_formulas
