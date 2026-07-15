// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Resolves a set of target components into an aggregate measurement formula.
//!
//! A component (inverter, CHP, ...) is measured by its own reading, or — when an
//! upstream meter measures exactly that component and its in-target siblings — by
//! that meter instead (the substitution in [`measurement_points`]). A meter is
//! measured by a `COALESCE` of its own reading and the sum of its children,
//! ordered by the [`SourcePreference`]. Per-child `COALESCE(_, 0)` fallbacks keep the
//! result total (it always resolves to a value).
//!
//! When several parallel meters feed one component group (a diamond), their
//! readings measure distinct feed lines and so sum to the group's throughput;
//! the group's own readings are the fallback (see [`diamond`]).

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, ComponentGraphConfig, Edge, Error, Node};
use std::collections::BTreeSet;

use super::expr::Expr;

/// How [`aggregate`] picks measurement sources.
///
/// The variants are ordered by how much a meter is trusted over the components
/// it measures; meter chains only make sense once meters are already primary, so
/// the otherwise-unreachable "components-first with chains" state is simply not
/// representable.
#[derive(Clone, Copy)]
pub(crate) enum SourcePreference {
    /// Component readings are the primary source; meter readings are the fallback.
    ComponentsFirst,
    /// Meter readings are the primary source; component readings are the fallback.
    MetersFirst,
    /// Like [`SourcePreference::MetersFirst`], but a meter may also be measured through a
    /// single (non-component) child meter instead of standing alone.
    MetersFirstWithChains,
}

impl SourcePreference {
    /// Meter readings primary when `prefer_meters`, component readings primary
    /// otherwise. (For meter chains, name [`SourcePreference::MetersFirstWithChains`].)
    pub(crate) fn prefer_meters(prefer_meters: bool) -> Self {
        if prefer_meters {
            SourcePreference::MetersFirst
        } else {
            SourcePreference::ComponentsFirst
        }
    }

    /// Whether meter readings are the primary source (components the fallback).
    fn prefers_meters(self) -> bool {
        matches!(
            self,
            SourcePreference::MetersFirst | SourcePreference::MetersFirstWithChains
        )
    }

    /// Whether a meter may be measured through a single (non-component) child meter.
    fn allows_meter_chains(self) -> bool {
        matches!(self, SourcePreference::MetersFirstWithChains)
    }
}

/// Builds the aggregate measurement formula for `targets`.
pub(crate) fn aggregate<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    targets: BTreeSet<u64>,
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let terms: Vec<Expr> = if graph.config.disable_fallback_components {
        targets.into_iter().map(Expr::component).collect()
    } else {
        measurement_points(graph, &targets)?
            .into_iter()
            .map(|point| match point {
                Measurement::Single(id) => measure(graph, id, policy),
                Measurement::Diamond { components, meters } => {
                    diamond(&components, &meters, policy)
                }
            })
            .collect::<Result<_, _>>()?
    };
    sum(terms).ok_or(Error::internal("No components to generate formula."))
}

/// A target resolved to a measurement source by [`measurement_points`].
#[derive(Debug, PartialEq)]
enum Measurement {
    /// A single node: a meter to drill into, or a component measured directly.
    Single(u64),
    /// A component group fed through several parallel meters. The meters sum to
    /// the group's throughput, with the component readings as the fallback; see
    /// [`diamond`].
    Diamond {
        components: Vec<u64>,
        meters: Vec<u64>,
    },
}

/// Resolves `targets` to the nodes actually measured: meters substituted in for
/// the component groups they exclusively measure, every other target kept as-is.
/// Ordered by the target that first reaches each node, so the sum is stable.
fn measurement_points<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    targets: &BTreeSet<u64>,
) -> Result<Vec<Measurement>, Error> {
    let mut remaining = targets.clone();
    let mut points = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some(id) = remaining.pop_first() {
        if let Some(substitution) = meter_substitution(graph, id, targets)? {
            for &sibling in &substitution.siblings {
                remaining.remove(&sibling);
            }
            if substitution.meters.len() > 1 {
                // Several parallel meters feed this group: combine them into one
                // diamond term rather than measuring each meter independently,
                // which would double-count the shared group.
                let mut components: Vec<u64> =
                    std::iter::once(id).chain(substitution.siblings).collect();
                components.sort_unstable();
                seen.extend(&substitution.meters);
                points.push(Measurement::Diamond {
                    components,
                    meters: substitution.meters,
                });
            } else {
                for meter in substitution.meters {
                    if seen.insert(meter) {
                        points.push(Measurement::Single(meter));
                    }
                }
            }
        } else if seen.insert(id) {
            points.push(Measurement::Single(id));
        }
    }
    Ok(points)
}

/// A component group measured through a meter: the predecessor meter(s) that
/// measure it, and the sibling ids they cover (which the caller then skips).
struct Substitution {
    meters: Vec<u64>,
    siblings: Vec<u64>,
}

/// If `id` is a component whose predecessor meter(s) exclusively measure it and
/// its siblings (all of which must be targets), returns those meter ids and the
/// covered sibling ids. Otherwise `None` (the component is measured directly).
fn meter_substitution<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    targets: &BTreeSet<u64>,
) -> Result<Option<Substitution>, Error> {
    let Some(meters) = parent_meters(graph, id)? else {
        return Ok(None);
    };
    // `siblings_from_predecessors` already excludes `id` itself and dedups.
    let siblings: Vec<u64> = graph
        .siblings_from_predecessors(id)?
        .map(|sibling| sibling.component_id())
        .collect();
    if !siblings.iter().all(|sibling| targets.contains(sibling)) {
        return Ok(None);
    }
    Ok(Some(Substitution {
        meters: meters.into_iter().collect(),
        siblings,
    }))
}

/// The predecessor meters directly measuring `id`, if `id` is a measurable
/// component fed by at least one meter. `None` when `id` is not a measurable
/// component or has no parent meter — in either case the meter substitution
/// does not apply.
///
/// An *internal* meter can also carry a phantom load (see
/// [`ComponentGraphConfig`]). A substitution would then count that load as
/// part of the group. This is a known, accepted trade-off: there is no
/// metadata that says a meter measures only its children, and the meter-side
/// term is only used when the group's own readings are already missing.
/// Without it, there would be no value at all.
fn parent_meters<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
) -> Result<Option<BTreeSet<u64>>, Error> {
    if !is_measurable_component(graph.component(id)?, &graph.config) {
        return Ok(None);
    }
    let meters: BTreeSet<u64> = graph
        .predecessors(id)?
        .filter(|predecessor| predecessor.is_meter())
        .map(|predecessor| predecessor.component_id())
        .collect();
    if meters.is_empty() {
        return Ok(None);
    }
    Ok(Some(meters))
}

/// The measurement expression for a single node.
fn measure<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let own = Expr::component(id);
    if !graph.component(id)?.is_meter() {
        // A component measured directly: its own reading, or 0.
        return Ok(own.coalesce(Expr::number(0.0)));
    }
    if stands_alone(graph, id, policy)? {
        return Ok(own);
    }

    let children: Vec<&N> = graph.successors(id)?.collect();
    let missing = || Error::internal("Meter that drills has no successors.");
    // `best` sums each child's reading-or-0 (child meters are assumed total),
    // so it always resolves.
    let best = sum(children.iter().map(|c| child_best_term(*c))).ok_or_else(missing)?;

    if policy.prefers_meters() {
        Ok(own.coalesce(best))
    } else {
        // `exact` is null unless every child reports.
        let exact =
            sum(children.iter().map(|c| Expr::component(c.component_id()))).ok_or_else(missing)?;
        // The last resort after `exact` and the meter:
        // - multiple children: `best`, the per-child reading-or-0 sum;
        // - a single device child: a plain 0 (`best` would just repeat the
        //   child already in `exact`);
        // - a single child meter: nothing — a meter is total on its own, so no
        //   trailing term is added.
        let last_resort = if children.len() > 1 {
            best
        } else if children[0].is_meter() {
            Expr::None
        } else {
            Expr::number(0.0)
        };
        Ok(exact.coalesce(own).coalesce(last_resort))
    }
}

/// The measurement for a component group fed through several parallel meters.
///
/// Each meter measures a distinct feed line into the group, so the meter
/// readings sum to the group's throughput. Ordered by the [`SourcePreference`]:
/// - meters primary: their exact sum, then the component readings, then a
///   per-meter best-effort sum (`COALESCE(#m, 0) + ...`) so the term is total;
/// - components primary: the component readings, then straight to that
///   best-effort sum — the exact meter sum it would otherwise carry in between
///   is dominated by the best-effort one, so it is omitted.
fn diamond(components: &[u64], meters: &[u64], policy: SourcePreference) -> Result<Expr, Error> {
    let empty = || Error::internal("Diamond measurement with no meters or components.");
    let component_sum = sum(components.iter().map(|&c| Expr::component(c))).ok_or_else(empty)?;
    let meter_best = sum(meters
        .iter()
        .map(|&m| Expr::coalesce(Expr::component(m), Expr::number(0.0))))
    .ok_or_else(empty)?;
    Ok(if policy.prefers_meters() {
        let meter_sum = sum(meters.iter().map(|&m| Expr::component(m))).ok_or_else(empty)?;
        meter_sum.coalesce(component_sum).coalesce(meter_best)
    } else {
        component_sum.coalesce(meter_best)
    })
}

/// A child's contribution to a meter's `best` sum: a child meter is assumed
/// total (`#id`); any other component falls back to 0 (`COALESCE(#id, 0)`).
fn child_best_term<N: Node>(child: &N) -> Expr {
    if child.is_meter() {
        Expr::component(child.component_id())
    } else {
        Expr::coalesce(Expr::component(child.component_id()), Expr::number(0.0))
    }
}

/// Whether a meter is measured by its own reading alone (rather than drilling
/// into its children).
fn stands_alone<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    policy: SourcePreference,
) -> Result<bool, Error> {
    let successors: Vec<&N> = graph.successors(id)?.collect();
    if successors.is_empty() {
        return Ok(true);
    }
    if !successors.iter().any(|successor| successor.is_meter()) {
        // No child meters: drill in and sum them.
        return Ok(false);
    }
    // Has a child meter: only drill through a single non-component child meter,
    // and only when meter chains are enabled.
    if !policy.allows_meter_chains() || successors.len() > 1 {
        return Ok(true);
    }
    graph.is_component_meter(successors[0].component_id())
}

fn is_measurable_component<N: Node>(node: &N, config: &ComponentGraphConfig) -> bool {
    node.is_battery_inverter(config)
        || node.is_chp()
        || node.is_pv_inverter()
        || node.is_ev_charger()
        || node.is_wind_turbine()
        || node.is_steam_boiler()
}

/// Sums the expressions, or `None` if there are none.
fn sum(exprs: impl IntoIterator<Item = Expr>) -> Option<Expr> {
    exprs.into_iter().reduce(|a, b| a + b)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{SourcePreference, aggregate};
    use crate::graph::test_utils::ComponentGraphBuilder;
    use crate::{ComponentGraphConfig, Error};

    /// Test fallback expression generation when there are no meters in the
    /// graph, with only PV inverters directly connected to the grid.
    #[test]
    fn test_no_meters() {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let inverter = builder.solar_inverter();
        builder.connect(grid, inverter);

        let graph = builder.build(None).unwrap();
        let expr = graph.pv_formula(None).unwrap().to_string();
        assert_eq!(expr, "COALESCE(#1, 0.0)");

        let inverter = builder.solar_inverter();
        builder.connect(grid, inverter);

        let graph = builder.build(None).unwrap();
        let expr = graph.pv_formula(None).unwrap().to_string();
        assert_eq!(expr, "COALESCE(#1, 0.0) + COALESCE(#2, 0.0)");
    }

    // ---------------------------------------------------------------
    // Pass-through scenarios.
    //
    // These tests document the *desired* behavior when a component
    // category that has no specific handling (here: `PowerTransformer`)
    // sits in the topology. Validators and the formula generator
    // should treat such a node as transparent — walking through it
    // instead of rejecting otherwise-valid neighbor relationships or
    // emitting it as a measurement source.
    // ---------------------------------------------------------------

    /// Validation accepts a graph where a pass-through category sits
    /// between an inverter and its meter / between a meter and the
    /// grid. Neighbor rules (`M1`, `I1-I4`, `B1`) consult the
    /// effective predecessors / successors, so the chain through the
    /// pass-through reads as if it weren't there.
    ///
    /// Topology: `Grid → PowerTransformer → Meter → BatteryInverter → Battery`
    #[test]
    fn test_validation_accepts_passthrough_predecessor() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, pt);
        builder.connect(pt, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        // Should build cleanly with the default config.
        let _graph = builder.build(None)?;
        Ok(())
    }

    /// A pass-through-only cycle attached to an otherwise-valid graph
    /// is rejected at construction time. The acyclicity validator
    /// walks the raw graph so cycles composed entirely of pass-through
    /// nodes are still detected.
    ///
    /// Topology: a normal `Grid → Meter → BatteryInverter → Battery`
    /// branch, plus a side-branch `Grid → PT1 → PT2 → PT3 → PT1` cycle.
    #[test]
    fn test_acyclicity_detects_passthrough_only_cycle() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        let pt1 = builder.power_transformer();
        let pt2 = builder.power_transformer();
        let pt3 = builder.power_transformer();

        builder.connect(grid, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        builder.connect(grid, pt1);
        builder.connect(pt1, pt2);
        builder.connect(pt2, pt3);
        builder.connect(pt3, pt1);

        assert!(
            builder.build(None).is_err(),
            "PT-only cycle reachable from the GCP must be detected at construction time"
        );
        Ok(())
    }

    /// A pass-through component preceding the GCP is tolerated: the
    /// effective predecessors view walks past it, so `ensure_root` sees
    /// no ancestor and accepts the GCP as a root.
    ///
    /// Topology: `PT → Grid → Meter → BatteryInverter → Battery`.
    #[test]
    fn test_ensure_root_tolerates_passthrough_predecessor() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(pt, grid);
        builder.connect(grid, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let _graph = builder.build(None)?;
        Ok(())
    }

    /// Grid formula skips a PowerTransformer that sits directly below
    /// the GCP and uses the meter beneath it as the measurement source
    /// — the formula is identical to the equivalent
    /// `Grid → Meter → Inverter → Battery` graph.
    ///
    /// Topology (component ids): `Grid:0 → PT:1 → Meter:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_grid_formula_skips_passthrough_at_root() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let pt = builder.power_transformer();
        let meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, pt);
        builder.connect(pt, meter);
        builder.connect(meter, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert!(
            !formula.contains("#1"),
            "PowerTransformer #1 must not appear in grid_formula, got {formula:?}",
        );
        assert_eq!(formula, "COALESCE(#2, #3, 0.0)");
        Ok(())
    }

    /// A meter is measured from its *effective* successors — walking past
    /// pass-throughs to the inverter rather than including the transformer
    /// (which has no measurement).
    ///
    /// Topology (component ids): `Grid:0 → Meter:1 → PT:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_grid_formula_skips_passthrough_successor() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let pt = builder.power_transformer();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, meter);
        builder.connect(meter, pt);
        builder.connect(pt, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?.to_string();
        assert!(
            !formula.contains("#2"),
            "PowerTransformer #2 must not appear in grid_formula, got {formula:?}",
        );
        assert_eq!(formula, "COALESCE(#1, #3, 0.0)");
        Ok(())
    }

    /// A component is measured through its predecessor meter, found by walking
    /// past a pass-through node.
    ///
    /// Topology (component ids): `Grid:0 → Meter:1 → PT:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_battery_formula_finds_meter_through_passthrough() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let pt = builder.power_transformer();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, meter);
        builder.connect(meter, pt);
        builder.connect(pt, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.battery_formula(None)?.to_string();
        // Inverter falls back to its effective predecessor meter,
        // walking past the transformer.
        assert_eq!(formula, "COALESCE(#1, #3, 0.0)");
        Ok(())
    }

    /// Aggregation over meters and components: meter substitution for component
    /// groups, the meter-vs-component source preference, meter chains, and the
    /// raw-sum shortcut under `disable_fallback_components`.
    #[test]
    fn test_aggregate() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // Add a battery meter with one inverter and one battery.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build(None)?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([1]),
            SourcePreference::MetersFirstWithChains,
        )?;
        assert_eq!(expr.to_string(), "#1");

        let expr = aggregate(
            &graph,
            BTreeSet::from([1, 2]),
            SourcePreference::MetersFirstWithChains,
        )?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

        let expr = aggregate(
            &graph,
            BTreeSet::from([1, 2]),
            SourcePreference::ComponentsFirst,
        )?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#3, #2, 0.0)");

        let expr = aggregate(
            &graph,
            BTreeSet::from([1, 2]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(expr.to_string(), "#1 + COALESCE(#2, #3, 0.0)");

        let expr = aggregate(&graph, BTreeSet::from([3]), SourcePreference::MetersFirst)?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
        let expr = aggregate(
            &graph,
            BTreeSet::from([3]),
            SourcePreference::MetersFirstWithChains,
        )?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");
        let expr = aggregate(
            &graph,
            BTreeSet::from([2]),
            SourcePreference::MetersFirstWithChains,
        )?;
        assert_eq!(expr.to_string(), "COALESCE(#2, #3, 0.0)");

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .disable_fallback_components(true)
                .build(),
        ))?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([1, 2]),
            SourcePreference::ComponentsFirst,
        )?;
        assert_eq!(expr.to_string(), "#1 + #2");

        let expr = aggregate(
            &graph,
            BTreeSet::from([1, 2]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(expr.to_string(), "#1 + #2");

        let expr = aggregate(&graph, BTreeSet::from([3]), SourcePreference::MetersFirst)?;
        assert_eq!(expr.to_string(), "#3");

        // Add a battery meter with three inverter and three batteries
        let meter_bat_chain = builder.meter_bat_chain(3, 3);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build(None)?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([3, 5]),
            SourcePreference::ComponentsFirst,
        )?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(",
                "#8 + #7 + #6, ",
                "#5, ",
                "COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)",
                ")"
            )
        );

        let expr = aggregate(
            &graph,
            BTreeSet::from([2, 5]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
            )
        );

        let expr = aggregate(
            &graph,
            BTreeSet::from([2, 6, 7, 8]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0))"
            )
        );

        let expr = aggregate(
            &graph,
            BTreeSet::from([2, 7, 8]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(
            expr.to_string(),
            "COALESCE(#2, #3, 0.0) + COALESCE(#7, 0.0) + COALESCE(#8, 0.0)"
        );

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .disable_fallback_components(true)
                .build(),
        ))?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([3, 5]),
            SourcePreference::ComponentsFirst,
        )?;
        assert_eq!(expr.to_string(), "#3 + #5");

        let expr = aggregate(
            &graph,
            BTreeSet::from([2, 5]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(expr.to_string(), "#2 + #5");

        let expr = aggregate(
            &graph,
            BTreeSet::from([2, 6, 7, 8]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(expr.to_string(), "#2 + #6 + #7 + #8");

        let expr = aggregate(
            &graph,
            BTreeSet::from([2, 7, 8]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(expr.to_string(), "#2 + #7 + #8");

        let meter = builder.meter();
        let chp = builder.chp();
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, meter);
        builder.connect(meter, chp);
        builder.connect(meter, pv_inverter);

        assert_eq!(meter.component_id(), 12);
        assert_eq!(chp.component_id(), 13);
        assert_eq!(pv_inverter.component_id(), 14);

        let graph = builder.build(None)?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([5, 12]),
            SourcePreference::MetersFirst,
        )?;
        assert_eq!(
            expr.to_string(),
            concat!(
                "COALESCE(#5, COALESCE(#8, 0.0) + COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) + ",
                "COALESCE(#12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0))"
            )
        );

        let expr = aggregate(
            &graph,
            BTreeSet::from([7, 14]),
            SourcePreference::ComponentsFirst,
        )?;
        assert_eq!(expr.to_string(), "COALESCE(#7, 0.0) + COALESCE(#14, 0.0)");

        Ok(())
    }

    /// Aggregation through a meter chain: a meter measured via its single
    /// (non-component) child meter when meter chains are enabled.
    #[test]
    fn test_aggregate_through_meter_chain() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let meter1 = builder.meter();
        let meter2 = builder.meter();
        let bat_chain = builder.meter_bat_chain(1, 1);
        let pv_chain = builder.meter_pv_chain(1);

        builder.connect(grid, meter1);
        builder.connect(meter1, meter2);
        builder.connect(meter2, bat_chain);
        builder.connect(meter2, pv_chain);

        let graph = builder.build(None)?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([meter1.component_id()]),
            SourcePreference::MetersFirstWithChains,
        )?;
        assert_eq!(expr.to_string(), "COALESCE(#1, #2)");

        Ok(())
    }

    /// `measurement_points` substitutes a component's predecessor meter for the
    /// component, but keeps a meterless component as its own measurement point.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Inverter:2 → Battery:3`, plus a
    /// meterless `Grid:0 → Inverter:4 → Battery:5`.
    #[test]
    fn test_measurement_points() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let metered = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid, meter);
        builder.connect(meter, metered);
        builder.connect(metered, battery);

        let meterless = builder.battery_inverter();
        let meterless_battery = builder.battery();
        builder.connect(grid, meterless);
        builder.connect(meterless, meterless_battery);

        let graph = builder.build(None)?;

        // The metered inverter (2) resolves to its meter (1)...
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([metered.component_id()]))?,
            vec![super::Measurement::Single(meter.component_id())],
        );
        // ...while the meterless inverter (4) is measured directly.
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([meterless.component_id()]))?,
            vec![super::Measurement::Single(meterless.component_id())],
        );
        Ok(())
    }

    /// A component with several parallel meter parents (a diamond) is measured
    /// once, through the combined meter readings, rather than double-counted by
    /// summing each meter independently.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1, Meter:2} → Inverter:3 → Battery:4`.
    #[test]
    fn test_aggregate_diamond() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let m1 = builder.meter();
        let m2 = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid, m1);
        builder.connect(grid, m2);
        builder.connect(m1, inverter);
        builder.connect(m2, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let targets = BTreeSet::from([inverter.component_id()]);

        // The two meters collapse into one diamond term over the inverter.
        assert_eq!(
            super::measurement_points(&graph, &targets)?,
            vec![super::Measurement::Diamond {
                components: vec![inverter.component_id()],
                meters: vec![m1.component_id(), m2.component_id()],
            }],
        );

        // Meters primary: their sum, then the inverter, then a best-effort sum.
        assert_eq!(
            aggregate(&graph, targets.clone(), SourcePreference::MetersFirst)?.to_string(),
            "COALESCE(#1 + #2, #3, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
        );
        // Components primary: the inverter, then the best-effort meter sum (the
        // exact meter sum is dominated by it and dropped).
        assert_eq!(
            aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
            "COALESCE(#3, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))",
        );
        Ok(())
    }

    /// A meter with only component children drills in; one with multiple
    /// successors that include a meter stands alone.
    ///
    /// Topology (ids): `Meter:1 → {Inverter:2, Inverter:3}` and
    /// `Meter:4 → {Meter:5, Inverter:6}`.
    #[test]
    fn test_stands_alone() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let component_meter = builder.meter();
        let inv1 = builder.battery_inverter();
        let inv2 = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid, component_meter);
        builder.connect(component_meter, inv1);
        builder.connect(component_meter, inv2);
        builder.connect(inv1, battery);
        builder.connect(inv2, battery);

        let mixed_meter = builder.meter();
        let child_meter = builder.meter();
        let inv3 = builder.battery_inverter();
        let battery2 = builder.battery();
        builder.connect(grid, mixed_meter);
        builder.connect(mixed_meter, child_meter);
        builder.connect(mixed_meter, inv3);
        builder.connect(inv3, battery2);

        let graph = builder.build(None)?;
        assert!(!super::stands_alone(
            &graph,
            component_meter.component_id(),
            SourcePreference::MetersFirst,
        )?);
        assert!(super::stands_alone(
            &graph,
            mixed_meter.component_id(),
            SourcePreference::MetersFirstWithChains,
        )?);
        Ok(())
    }
}
