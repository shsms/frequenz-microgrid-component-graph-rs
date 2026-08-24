// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Graph predicates for the fallback engine.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, ComponentGraphConfig, Edge, Error, Node};

/// The predecessor meters directly measuring `id`. `None` when `id` is not a
/// measurable component, has no parent meter, or is fed by a grid meter or a
/// meter that provides no telemetry; then neither the meter substitution nor
/// the subtraction applies, and the component is measured directly instead. A
/// grid meter (see [`is_grid_meter`]) carries the site's unmodeled consumer
/// load, so its reading covers more than its graph children: it can neither
/// stand in for them nor be split across them. A no-telemetry meter has no
/// reading to stand in or be split at all.
///
/// An *internal* meter can also carry a phantom load (see
/// [`ComponentGraphConfig`]). A substitution or subtraction would then count
/// that load as part of the group. This is a known, accepted trade-off: there
/// is no metadata that says a meter measures only its children. With
/// components first, the meter-side term only fills in when the group's own
/// readings are missing. With meters first, it is the primary source, so the
/// phantom load is counted whenever the meter reports.
pub(crate) fn parent_meters<N: Node, E: Edge>(
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
    for &meter in &meters {
        let meter = graph.component(meter)?;
        if is_grid_meter(graph, meter)? || !meter.provides_telemetry() {
            return Ok(None);
        }
    }
    Ok(Some(meters))
}

/// Whether `id` is reached only through the given `meters`: every
/// predecessor is one of the meters, or is itself a meter reached only
/// through them (a nested feed). Then the meters' readings account for
/// `id`'s full throughput. A feed from outside the meters (another meter,
/// the grid, or an unmodeled source) is not in those readings, so a sum or
/// difference over them would miscount it. [`outside_feeds`] names the
/// disqualifying feeds; keep the two in lockstep.
pub(crate) fn reached_only_through<N: Node, E: Edge>(
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

/// The membership test [`reaches_any_below`] and [`reached_below`] share: a
/// member of `set` other than the starting node itself (`id` may be in
/// `set`).
fn below_member<N: Node>(id: u64, set: &BTreeSet<u64>) -> impl Fn(&N) -> bool + '_ {
    move |node| node.component_id() != id && set.contains(&node.component_id())
}

/// Whether `id` reaches any member of `set` strictly below itself, following
/// the feed lines downward. [`reached_below`] names the reached members.
pub(crate) fn reaches_any_below<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    set: &BTreeSet<u64>,
) -> Result<bool, Error> {
    graph.reaches_any(id, below_member(id, set), petgraph::Direction::Outgoing)
}

/// The members of `set` that `id` reaches strictly below itself, following
/// the feed lines downward — [`reaches_any_below`] with the reached members
/// named, so an explanation can say who already carries the flow. Shares
/// that predicate, so the two cannot drift apart; ascending, as the ids are
/// rendered into rationale text.
pub(super) fn reached_below<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    set: &BTreeSet<u64>,
) -> Result<Vec<u64>, Error> {
    Ok(graph
        .find_all(
            id,
            below_member(id, set),
            petgraph::Direction::Outgoing,
            true,
        )?
        .into_iter()
        .collect())
}

/// The feeds into `id` that disqualify it from [`reached_only_through`]: each
/// predecessor that is not one of the `meters`, and is either a non-meter or
/// a meter itself fed from outside them. Empty exactly when
/// [`reached_only_through`] holds, so an explanation can name the parallel
/// feeds.
pub(super) fn outside_feeds<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    meters: &BTreeSet<u64>,
) -> Result<Vec<u64>, Error> {
    let mut feeds = Vec::new();
    for predecessor in graph.predecessors(id)? {
        let predecessor_id = predecessor.component_id();
        if meters.contains(&predecessor_id) {
            continue;
        }
        if !predecessor.is_meter() || !reached_only_through(graph, predecessor_id, meters)? {
            feeds.push(predecessor_id);
        }
    }
    Ok(feeds)
}

/// Returns true if the node is a grid meter.
///
/// A given component is identified as a grid meter if:
///  - it is a meter,
///  - it is not a component meter (battery meter, pv meter, etc.),
///  - one of its predecessors is the grid connection point (or, recursively,
///    it is the sole child of such a meter — a fallback grid meter).
///
/// Every predecessor is checked, so the answer does not depend on edge order
/// when a meter has several feeds (possible only when validation failures
/// are allowed).
pub(crate) fn is_grid_meter<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    component: &N,
) -> Result<bool, Error> {
    is_grid_meter_inner(graph, component, &mut BTreeSet::new())
}

/// [`is_grid_meter`] with the set of components already being checked. A
/// repeated component cannot prove a grid connection again: skipping it stops
/// the recursion on a cyclic graph (possible only when validation failures
/// are allowed) instead of overflowing the stack.
fn is_grid_meter_inner<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    component: &N,
    checking: &mut BTreeSet<u64>,
) -> Result<bool, Error> {
    let id = component.component_id();
    if !checking.insert(id) || !component.is_meter() || graph.is_component_meter(id)? {
        return Ok(false);
    }
    let has_no_siblings = graph.siblings_from_predecessors(id)?.next().is_none();
    for predecessor in graph.predecessors(id)? {
        if predecessor.is_grid()
            || (has_no_siblings && is_grid_meter_inner(graph, predecessor, checking)?)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn is_measurable_component<N: Node>(node: &N, config: &ComponentGraphConfig) -> bool {
    node.is_battery_inverter(config)
        || node.is_chp()
        || node.is_pv_inverter()
        || node.is_ev_charger()
        || node.is_wind_turbine()
        || node.is_steam_boiler()
}

/// The subset of `ids` whose components provide telemetry, in the given order.
///
/// A component that provides no telemetry has no reading to emit, so it is
/// dropped from any sum or difference of component readings; the meter that
/// measures it still stands in for it.
pub(crate) fn ids_with_telemetry<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    ids: impl IntoIterator<Item = u64>,
) -> Result<Vec<u64>, Error> {
    let mut kept = Vec::new();
    for id in ids {
        if graph.component(id)?.provides_telemetry() {
            kept.push(id);
        }
    }
    Ok(kept)
}
