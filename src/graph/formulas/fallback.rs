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
//!
//! A meter's children can be a mix: some are targets, others are measured
//! nodes that are not targets. Those others can be sibling meters (e.g. a
//! PV+battery meter over PV inverters and a battery sub-meter) or non-target
//! components (e.g. one unreachable inverter next to its working siblings).
//! In that case the targets are measured as the parent meter minus those
//! siblings, with the component readings as the fallback (see
//! [`subtraction_term`]).

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
    sum(aggregate_terms(graph, targets, policy)?)
        .ok_or(Error::internal("No components to generate formula."))
}

/// The per-group measurement terms for `targets`: one [`Expr`] per measurement
/// point. A point is a meter substituted in for the group it exclusively
/// measures, a diamond, a subtraction, or a single component. [`aggregate`]
/// sums these terms. Callers that subtract the groups (e.g. the consumer
/// formula) keep them separate. This way, siblings that share a meter are
/// measured as one group; no sibling pulls in the others as a difference.
pub(crate) fn aggregate_terms<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    targets: BTreeSet<u64>,
    policy: SourcePreference,
) -> Result<Vec<Expr>, Error> {
    if graph.config.disable_fallback_components {
        Ok(targets.into_iter().map(Expr::component).collect())
    } else {
        measurement_points(graph, &targets)?
            .into_iter()
            .map(|point| match point {
                Measurement::Single(id) => measure(graph, id, policy),
                Measurement::Diamond { components, meters } => {
                    diamond(&components, &meters, policy)
                }
                Measurement::Subtraction {
                    parent_meters,
                    subtracted,
                    components,
                } => subtraction_term(&parent_meters, &subtracted, &components, policy),
            })
            .collect()
    }
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
    /// A component group measured as its parent meter(s) minus their other
    /// children (sibling meters or non-target components); see [`subtraction_term`].
    Subtraction {
        /// The parent meter(s) whose readings sum to cover the whole group.
        parent_meters: Vec<u64>,
        /// The parents' other children — sibling meters or measurable
        /// non-target components — whose readings are subtracted from the
        /// parent-meter sum, leaving just what flows through `components`.
        subtracted: Vec<u64>,
        /// The target components this point measures. Their value is what
        /// remains of the parent-meter sum after subtracting every id in
        /// `subtracted`.
        components: Vec<u64>,
    },
}

/// Called when a meter point that covers the `subsumed` nodes is emitted.
/// Drops any standalone `Single` points already emitted for those nodes.
/// Such a point can exist: a component sub-meter target can get its own
/// `Single` first, and only a later sibling reveals that their shared parent
/// meter covers the whole group. Leaving both in would subtract its readings
/// twice. A dropped sub-meter stays in `seen`: its flow is inside the
/// covering point now, so a later seed must not claim it again.
fn drop_subsumed(points: &mut Vec<Measurement>, subsumed: &[u64]) {
    points.retain(|point| {
        !matches!(point, Measurement::Single(single) if subsumed.contains(single))
    });
}

/// Resolves `targets` to the measurement points that cover them: a meter
/// substituted in for the group it exclusively measures, a diamond or a
/// subtraction for a group next to siblings, and a `Single` for every other
/// target. Ordered by the target that first reaches each node, so the sum is
/// stable.
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
            // A sibling can itself be a component sub-meter (e.g. a battery
            // meter under a mixed meter). If its id was resolved first, it may
            // already have its own point: a meter can't be a substitution
            // seed, so it fell through to a standalone `Single`. The new meter
            // point covers it too, so drop that `Single` to avoid subtracting
            // its readings twice.
            drop_subsumed(&mut points, &substitution.siblings);
            if substitution.meters.len() > 1 {
                // Several parallel meters feed this group: combine them into one
                // diamond term rather than measuring each meter independently,
                // which would double-count the shared group.
                seen.extend(&substitution.meters);
                points.push(Measurement::Diamond {
                    components: group_components(id, substitution.siblings),
                    meters: substitution.meters,
                });
            } else {
                for meter in substitution.meters {
                    if seen.insert(meter) {
                        points.push(Measurement::Single(meter));
                    }
                }
            }
        } else {
            match meter_subtraction(graph, id, targets)? {
                // A parent meter already claimed by an earlier group measures
                // more than this one, so the subtraction only applies while its
                // parent meters are all unclaimed.
                Some(sub) if sub.parent_meters.iter().all(|meter| !seen.contains(meter)) => {
                    for &component in &sub.components {
                        remaining.remove(&component);
                    }
                    seen.extend(&sub.parent_meters);
                    points.push(Measurement::Subtraction {
                        parent_meters: sub.parent_meters,
                        subtracted: sub.subtracted,
                        components: sub.components,
                    });
                }
                _ => {
                    if seen.insert(id) {
                        points.push(Measurement::Single(id));
                    }
                }
            }
        }
    }
    Ok(points)
}

/// The full target group a measurement point covers: the seed component plus
/// the sibling targets it subsumes, sorted for a stable term order.
fn group_components(seed: u64, siblings: Vec<u64>) -> Vec<u64> {
    let mut components: Vec<u64> = std::iter::once(seed).chain(siblings).collect();
    components.sort_unstable();
    components
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
/// component or has no parent meter — in either case neither the meter
/// substitution nor the subtraction applies.
///
/// An *internal* meter can also carry a phantom load (see
/// [`ComponentGraphConfig`]). A substitution or subtraction would then count
/// that load as part of the group. This is a known, accepted trade-off: there
/// is no metadata that says a meter measures only its children. With
/// components first, the meter-side term only fills in when the group's own
/// readings are missing. With meters first, it is the primary source, so the
/// phantom load is counted whenever the meter reports.
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

/// Whether `id` is reached only through the given `meters`: every
/// predecessor is one of the meters, or is itself a meter reached only
/// through them (a nested feed). Then the meters' readings account for
/// `id`'s full throughput. A feed from outside the meters (another meter,
/// the grid, or an unmodeled source) is not in those readings, so a sum or
/// difference over them would miscount it.
fn reached_only_through<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    meters: &BTreeSet<u64>,
) -> Result<bool, Error> {
    reached_only_through_inner(graph, id, meters, &mut BTreeSet::new())
}

/// [`reached_only_through`] with the set of predecessors already checked. A
/// predecessor in the set counts as reached only through the meters: its check has
/// already passed, or is still running higher up the call chain. Skipping it
/// keeps the walk linear on diamonds and stops the recursion on a cyclic
/// graph (possible only when validation failures are allowed) instead of
/// overflowing the stack.
fn reached_only_through_inner<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    meters: &BTreeSet<u64>,
    checked: &mut BTreeSet<u64>,
) -> Result<bool, Error> {
    for predecessor in graph.predecessors(id)? {
        let predecessor_id = predecessor.component_id();
        if meters.contains(&predecessor_id) || !checked.insert(predecessor_id) {
            continue;
        }
        if !predecessor.is_meter()
            || !reached_only_through_inner(graph, predecessor_id, meters, checked)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether the seed and every sibling in its group are reached only through
/// `meters`. If so, the meter readings cover the group's full throughput. In
/// an asymmetric diamond, a sibling that is also fed from outside these
/// meters fails this check. The group then resolves through that sibling
/// instead: its parent meters cover the whole group, and any point already
/// emitted for this seed is dropped as covered.
fn group_reached_only_through<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    siblings: &[u64],
    meters: &BTreeSet<u64>,
) -> Result<bool, Error> {
    for &component in std::iter::once(&id).chain(siblings) {
        if !reached_only_through(graph, component, meters)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `id` reaches any member of `set` strictly below itself, following
/// the feed lines downward. `find_all` starts at the node itself, so it is
/// excluded explicitly (`id` may be in `set`).
fn reaches_any_below<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    set: &BTreeSet<u64>,
) -> Result<bool, Error> {
    Ok(!graph
        .find_all(
            id,
            |node| node.component_id() != id && set.contains(&node.component_id()),
            petgraph::Direction::Outgoing,
            false,
        )?
        .is_empty())
}

/// A component group measured as its parent meter(s) minus their other
/// children, found by [`meter_subtraction`]. The fields mirror
/// [`Measurement::Subtraction`], which the caller builds from this.
struct Subtraction {
    /// The parent meter(s) whose readings sum to cover the whole group.
    parent_meters: Vec<u64>,
    /// The parents' other children, subtracted from the parent-meter sum.
    subtracted: Vec<u64>,
    /// The target components the measurement covers.
    components: Vec<u64>,
}

/// If `id` is a component under a single meter whose non-target children all
/// have their own readings (meters or measurable components) — none of them
/// leading to further targets, and each fed only through that parent — the
/// targets are measured as the parent meter minus those siblings (e.g. a
/// PV+battery meter minus the battery sub-meter, or one unreachable inverter
/// measured as the meter minus its working siblings), returned as the
/// [`Subtraction`] covering the whole group. Otherwise `None` (the component
/// is measured some other way).
fn meter_subtraction<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    targets: &BTreeSet<u64>,
) -> Result<Option<Subtraction>, Error> {
    let Some(meters) = parent_meters(graph, id)? else {
        return Ok(None);
    };
    if meters.len() > 1 {
        // Several parallel parent meters: left to the diamond handling.
        return Ok(None);
    }
    for &meter in &meters {
        if graph
            .predecessors(meter)?
            .any(|predecessor| predecessor.is_grid())
        {
            // A meter directly under the grid connection point carries the
            // site's unmodeled consumer load (the consumer formula counts its
            // residual), so its reading is not exhaustive over its graph
            // children.
            return Ok(None);
        }
    }
    let mut siblings = Vec::new();
    let mut minus = Vec::new();
    for sibling in graph.siblings_from_predecessors(id)? {
        let sibling_id = sibling.component_id();
        if targets.contains(&sibling_id) {
            if sibling.is_meter() {
                // A target meter sibling: the parent's reading can't be split
                // cleanly.
                return Ok(None);
            }
            siblings.push(sibling_id);
        } else if sibling.is_meter() || is_measurable_component(sibling, &graph.config) {
            minus.push(sibling_id);
        } else {
            // A sibling with no usable reading: its share of the parent's
            // reading is unknown.
            return Ok(None);
        }
    }
    if minus.is_empty() {
        // Every sibling is a target: `meter_substitution` covers this.
        return Ok(None);
    }
    // The subtracted siblings must measure only what flows through the parent
    // meters, and must not lead to other targets (those are measured on their
    // own, so subtracting them here would drop them from the total). A sibling
    // that is itself one of the parent meters is rejected here too: the seed
    // below it is always a nested target.
    for &subtracted in &minus {
        if !reached_only_through(graph, subtracted, &meters)? {
            return Ok(None);
        }
        if reaches_any_below(graph, subtracted, targets)? {
            return Ok(None);
        }
    }
    // A subtracted sibling can feed another subtracted sibling — e.g. a
    // sub-meter next to the inverter it feeds, both under the parent meter.
    // The feeder's reading measures flow that is already inside the fed
    // sibling's own reading, so subtracting both would subtract that flow
    // twice. If all of a feeder's children are subtracted too, they fully
    // cover its reading, and the feeder is dropped from the difference. Any
    // other overlap can't be split cleanly, so the subtraction does not apply.
    let minus_set: BTreeSet<u64> = minus.iter().copied().collect();
    let mut covered = Vec::new();
    for &subtracted in &minus {
        if !reaches_any_below(graph, subtracted, &minus_set)? {
            continue;
        }
        if graph.has_successors(subtracted)?
            && graph
                .successors(subtracted)?
                .all(|child| minus_set.contains(&child.component_id()))
        {
            covered.push(subtracted);
        } else {
            return Ok(None);
        }
    }
    minus.retain(|subtracted| !covered.contains(subtracted));
    // The seed and every covered target must be reached only through the
    // parent meters, so the parent-meter sum accounts for its full
    // throughput — the same guard the subtracted siblings pass above.
    if !group_reached_only_through(graph, id, &siblings, &meters)? {
        return Ok(None);
    }
    minus.sort_unstable();
    Ok(Some(Subtraction {
        parent_meters: meters.into_iter().collect(),
        subtracted: minus,
        components: group_components(id, siblings),
    }))
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
    let best = sum(children.iter().map(|c| child_best_effort_term(*c))).ok_or_else(missing)?;

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

/// The exact sum of the nodes' readings (`#a + #b + ...`): null unless every
/// one reports. `None` when `ids` is empty.
fn exact_sum(ids: &[u64]) -> Option<Expr> {
    sum(ids.iter().map(|&id| Expr::component(id)))
}

/// The best-effort sum of the nodes' readings (`COALESCE(#a, 0) + ...`): each
/// reading or 0, so it always resolves. `None` when `ids` is empty.
fn best_effort_sum(ids: &[u64]) -> Option<Expr> {
    sum(ids
        .iter()
        .map(|&id| Expr::coalesce(Expr::component(id), Expr::number(0.0))))
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
    let component_sum = exact_sum(components).ok_or_else(empty)?;
    let meter_best = best_effort_sum(meters).ok_or_else(empty)?;
    Ok(if policy.prefers_meters() {
        let meter_sum = exact_sum(meters).ok_or_else(empty)?;
        meter_sum.coalesce(component_sum).coalesce(meter_best)
    } else {
        component_sum.coalesce(meter_best)
    })
}

/// The measurement for a component group measured as its parent meter(s) minus
/// their other children.
///
/// The difference measures exactly the group: everything through the parent
/// meters except what the subtracted siblings account for. Several parallel
/// parent meters (a diamond) sum to the group's throughput. The siblings are
/// subtracted by their bare readings — a `COALESCE(_, 0)` there would
/// attribute a missing sibling's power to the group. Ordered by the
/// [`SourcePreference`]:
/// - meters primary: the difference, then the per-component reading-or-0 sum;
/// - components primary: the exact component sum, then the difference, then
///   that reading-or-0 sum (or a plain 0 for a single component, which `exact`
///   already covers).
fn subtraction_term(
    parent_meters: &[u64],
    subtracted: &[u64],
    components: &[u64],
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let empty = || Error::internal("Subtraction measurement with no components.");
    let no_meters = || Error::internal("Subtraction measurement with no parent meters.");
    // Several parallel parent meters (a diamond) sum to the group's throughput;
    // a single meter is just that sum of one.
    let meter_sum = exact_sum(parent_meters).ok_or_else(no_meters)?;
    let difference = subtracted
        .iter()
        .fold(meter_sum, |expr, &m| expr - Expr::component(m));
    let exact = exact_sum(components).ok_or_else(empty)?;
    let best = best_effort_sum(components).ok_or_else(empty)?;
    Ok(if policy.prefers_meters() {
        difference.coalesce(best)
    } else {
        let last_resort = if components.len() > 1 {
            best
        } else {
            Expr::number(0.0)
        };
        exact.coalesce(difference).coalesce(last_resort)
    })
}

/// A child's contribution to a meter's `best` sum: a child meter is assumed
/// total (`#id`); any other component falls back to 0 (`COALESCE(#id, 0)`).
fn child_best_effort_term<N: Node>(child: &N) -> Expr {
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
        assert_eq!(formula, "COALESCE(#3, #1, 0.0)");
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
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5 - #6, COALESCE(#7, 0.0) + COALESCE(#8, 0.0))"
            )
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
        assert_eq!(
            expr.to_string(),
            "COALESCE(#7, #5 - #6 - #8, 0.0) + COALESCE(#14, #12 - #13, 0.0)"
        );

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

    /// A meter over a mix of target components and other meters measures the
    /// targets with the meter minus the sibling meters as the meter-side
    /// source, ordered against the component readings by the policy.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3..7 (PV),
    /// Meter:8}` — a "PV + unspecified" meter next to an unspecified sub-meter.
    #[test]
    fn test_aggregate_subtraction() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let inverters: Vec<_> = (0..5).map(|_| builder.solar_inverter()).collect();
        for inverter in &inverters {
            builder.connect(mixed_meter, *inverter);
        }
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);

        let graph = builder.build(None)?;
        let targets = BTreeSet::from([3, 4, 5, 6, 7]);

        // The group collapses into one subtraction term over the mixed meter.
        assert_eq!(
            super::measurement_points(&graph, &targets)?,
            vec![super::Measurement::Subtraction {
                parent_meters: vec![mixed_meter.component_id()],
                subtracted: vec![sub_meter.component_id()],
                components: vec![3, 4, 5, 6, 7],
            }],
        );

        // Meters primary: the difference, then the per-inverter sum.
        assert_eq!(
            aggregate(&graph, targets.clone(), SourcePreference::MetersFirst)?.to_string(),
            concat!(
                "COALESCE(#2 - #8, ",
                "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
                "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
            ),
        );
        // Components primary: the exact sum, the difference, then best-effort.
        assert_eq!(
            aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
            concat!(
                "COALESCE(#3 + #4 + #5 + #6 + #7, #2 - #8, ",
                "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
                "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
            ),
        );

        // The PV formula resolves to the subtraction term.
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            concat!(
                "COALESCE(#3 + #4 + #5 + #6 + #7, #2 - #8, ",
                "COALESCE(#3, 0.0) + COALESCE(#4, 0.0) + COALESCE(#5, 0.0) + ",
                "COALESCE(#6, 0.0) + COALESCE(#7, 0.0))"
            ),
        );
        // A partial group (one inverter without its mates) falls back to the
        // meter minus everything else under it — the working siblings and the
        // sub-meter.
        assert_eq!(
            graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
            "COALESCE(#3, #2 - #4 - #5 - #6 - #7 - #8, 0.0)",
        );
        Ok(())
    }

    /// The subtracted siblings may be components rather than meters: a single
    /// target inverter (e.g. one that is unreachable over the network) falls
    /// back to the meter minus its working siblings, and a category's
    /// inverters next to another category's inverter fall back to the meter
    /// minus that inverter.
    #[test]
    fn test_aggregate_subtraction_component_siblings() -> Result<(), Error> {
        // One inverter out of three: the meter minus the others as fallback.
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → Inverter:3..5 (PV)`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let pv_meter = builder.meter_pv_chain(3);
        builder.connect(grid, main_meter);
        builder.connect(main_meter, pv_meter);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.pv_formula(Some(BTreeSet::from([3])))?.to_string(),
            "COALESCE(#3, #2 - #4 - #5, 0.0)",
        );

        // A PV inverter next to a battery inverter: each category is the
        // meter minus the other category's inverter.
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
        // Inverter:4 → Battery:5}`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let battery_inverter = builder.inv_bat_chain(1);
        builder.connect(mixed_meter, battery_inverter);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#3, #2 - #4, 0.0)",
        );
        assert_eq!(
            graph.battery_formula(None)?.to_string(),
            "COALESCE(#4, #2 - #3, 0.0)",
        );
        Ok(())
    }

    /// The subtraction also applies when the sibling meter is a component
    /// meter, in either order: PV inverters next to a battery sub-meter get
    /// `mixed - battery_meter` as their meter-side source, and battery
    /// inverters next to a PV sub-meter get `mixed - pv_meter`. The
    /// sub-meter's own category formula is unaffected.
    #[test]
    fn test_aggregate_subtraction_component_sub_meter() -> Result<(), Error> {
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3,4 (PV),
        // Meter:5 → Inverter:6 → Battery:7}`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let pv1 = builder.solar_inverter();
        let pv2 = builder.solar_inverter();
        builder.connect(mixed_meter, pv1);
        builder.connect(mixed_meter, pv2);
        let battery_meter = builder.meter_bat_chain(1, 1);
        builder.connect(mixed_meter, battery_meter);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#3 + #4, #2 - #5, COALESCE(#3, 0.0) + COALESCE(#4, 0.0))",
        );
        assert_eq!(
            graph.battery_formula(None)?.to_string(),
            "COALESCE(#6, #5, 0.0)",
        );

        // The reverse order: battery inverters under the mixed meter, the PV
        // system behind the sub-meter.
        //
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 →
        // Battery:4, Meter:5 → Inverter:6 (PV)}`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let battery_inverter = builder.inv_bat_chain(1);
        builder.connect(mixed_meter, battery_inverter);
        let pv_meter = builder.meter_pv_chain(1);
        builder.connect(mixed_meter, pv_meter);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.battery_formula(None)?.to_string(),
            "COALESCE(#3, #2 - #5, 0.0)",
        );
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#6, #5, 0.0)");
        Ok(())
    }

    /// Shapes where the parent meter's reading can't be split cleanly fall
    /// back to measuring the targets directly:
    /// - a non-target sibling with no usable reading (its share is unknown);
    /// - a sibling meter leading to other targets (they are measured on their
    ///   own, so subtracting them would drop them from the total);
    /// - a parent meter directly under the grid connection point (it carries
    ///   the site's unmodeled consumer load);
    /// - a sibling meter that is also fed from outside the parent.
    #[test]
    fn test_subtraction_disqualifiers() -> Result<(), Error> {
        // Non-target sibling with no usable reading (a hybrid inverter is not
        // a measurable component).
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
        // Inverter:4 (hybrid) → Battery:5}`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let hybrid = builder.add_component(crate::ComponentCategory::Inverter(
            crate::InverterType::Hybrid,
        ));
        let battery = builder.battery();
        builder.connect(mixed_meter, hybrid);
        builder.connect(hybrid, battery);

        let graph = builder.build(None)?;
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

        // Sibling meter leading to another target.
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
        // Meter:4 → Inverter:5 (PV)}`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let pv_meter = builder.meter_pv_chain(1);
        builder.connect(mixed_meter, pv_meter);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#3, 0.0) + COALESCE(#5, #4, 0.0)",
        );

        // Parent meter directly under the grid connection point.
        // Topology (ids): `Grid:0 → Meter:1 → {Inverter:2 (PV), Meter:3}`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let pv = builder.solar_inverter();
        builder.connect(grid_meter, pv);
        let sub_meter = builder.meter();
        builder.connect(grid_meter, sub_meter);

        let graph = builder.build(None)?;
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#2, 0.0)");

        // Sibling meter also fed from outside the parent.
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
        // Meter:4}`, plus `Meter:1 → Meter:4`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);
        builder.connect(main_meter, sub_meter);

        let graph = builder.build(None)?;
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
        Ok(())
    }
}
