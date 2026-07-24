// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Resolves target ids into the measurement points the formula sums.

use std::collections::{BTreeMap, BTreeSet};

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};

use super::predicates::{
    is_measurable_component, parent_meters, reached_only_through, reaches_any_below,
};

/// A target resolved to a measurement source by [`measurement_points`].
#[derive(Debug, PartialEq)]
pub(super) enum Measurement {
    /// A single node: a meter to drill into, or a component measured directly.
    Single(u64),
    /// A component group fed through several parallel meters. The meters sum to
    /// the group's throughput, with the component readings as the fallback; see
    /// [`diamond_term`](super::emit::diamond_term).
    Diamond {
        components: Vec<u64>,
        meters: Vec<u64>,
    },
    /// A component group measured as its parent meter(s) minus their other
    /// children (sibling meters or non-target components); see [`subtraction_term`](super::emit::subtraction_term).
    /// Several parallel parent meters (a diamond) sum to the group's throughput.
    Subtraction {
        /// The parent meter(s) whose readings sum to cover the whole group;
        /// several in parallel form a diamond.
        parent_meters: Vec<u64>,
        /// The parents' other children — sibling meters or measurable
        /// non-target components — whose readings are subtracted from the
        /// parent-meter sum, leaving just what flows through `components`.
        /// Empty when the redundant-feeder pruning dropped every sibling;
        /// the point is then just the parent-meter sum.
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
/// twice. A dropped sub-meter stays claimed: its flow is inside the covering
/// point now, so a later seed must not claim it again.
fn drop_subsumed(points: &mut Vec<Measurement>, subsumed: &[u64]) {
    points
        .retain(|point| !matches!(point, Measurement::Single(single) if subsumed.contains(single)));
}

/// Resolves `targets` to the measurement points that cover them: a meter
/// substituted in for the group it exclusively measures, a diamond or a
/// subtraction for a group next to siblings, and a `Single` for every other
/// target. Ordered by the target that first reaches each node, so the sum is
/// stable.
///
/// `claimed` holds every node an emitted point already accounts for: covered
/// meters and standalone nodes. It gates duplicate emission (a group whose
/// single meter is claimed merges silently; a subtraction with a claimed
/// parent falls back to a standalone point). A claim is never released:
/// [`drop_subsumed`] keeps the retracted points' nodes claimed.
pub(super) fn measurement_points<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    targets: &BTreeSet<u64>,
) -> Result<Vec<Measurement>, Error> {
    let mut remaining = targets.clone();
    let mut points = Vec::new();
    let mut claimed = BTreeSet::new();
    // Groups already resolved, keyed by parent-meter set; see [`classify`]
    // for why one result fits every seed.
    let mut groups = BTreeMap::new();
    while let Some(id) = remaining.pop_first() {
        let group = match classify(graph, id, targets, &mut groups)? {
            // A parent meter already claimed by an earlier point measures
            // more than this group, so a subtraction only applies while its
            // parent meters are all unclaimed. A group the meters measure
            // exactly is not gated on that: a single meter merges into
            // the claiming point below, and a diamond's meters are never
            // claimed first by the targets the current callers build.
            Some(group)
                if group.subtracted.is_none()
                    || group.meters.iter().all(|meter| !claimed.contains(meter)) =>
            {
                group
            }
            _ => {
                if claimed.insert(id) {
                    points.push(Measurement::Single(id));
                }
                continue;
            }
        };
        for &component in &group.components {
            remaining.remove(&component);
        }
        // A covered target may already have been measured on its own: a
        // component sub-meter can't be a classify seed and lands as a
        // standalone `Single` when its id is resolved first, and in an
        // asymmetric diamond only the seed fed by every parallel meter sees
        // the full parent-meter set. This group subsumes such a point, so
        // drop it to avoid counting its readings twice.
        drop_subsumed(&mut points, &group.components);
        if let Some(subtracted) = group.subtracted {
            claimed.extend(&group.meters);
            points.push(Measurement::Subtraction {
                parent_meters: group.meters,
                subtracted,
                components: group.components,
            });
        } else if group.meters.len() > 1 {
            // Several parallel meters feed this group: combine them into one
            // diamond term rather than measuring each meter independently,
            // which would double-count the shared group.
            claimed.extend(&group.meters);
            points.push(Measurement::Diamond {
                components: group.components,
                meters: group.meters,
            });
        } else {
            // A single meter measuring exactly this group: the group merges
            // into the meter's own point (deduped against an earlier claim).
            for meter in group.meters {
                if claimed.insert(meter) {
                    points.push(Measurement::Single(meter));
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

/// One resolved measurement group for a seed target: the parent meter(s)
/// measuring it, the targets they cover, and the sibling readings subtracted
/// from the meter sum.
#[derive(Clone)]
struct Group {
    /// The parent meter(s) whose readings sum to cover the whole group;
    /// several in parallel form a diamond. Sorted.
    meters: Vec<u64>,
    /// The covered targets: the seed plus its covered sibling targets. Sorted.
    components: Vec<u64>,
    /// The parents' other children, subtracted from the meter sum. Sorted.
    /// `None` when the meters measure exactly the covered targets. The
    /// redundant-feeder pruning can empty the list; the group keeps its
    /// subtraction shape then.
    subtracted: Option<Vec<u64>>,
}

/// Resolves the group that measures `seed` through its parent meter(s), if
/// there is one: the meters (one, or several in parallel — a diamond), the
/// sibling targets they cover, and the non-target siblings subtracted from
/// the summed meter readings. With no non-target siblings the meters measure
/// exactly the covered group (e.g. a battery meter over its inverters); with
/// non-target siblings the group is the meter sum minus those siblings (e.g.
/// a PV+battery meter minus the battery sub-meter, or one unreachable
/// inverter measured as the meter minus its working siblings). `None` when no
/// sound group exists; the seed is then measured directly.
///
/// The outcome depends only on the parent-meter set, not on the seed, so
/// the result is cached by that set. A seed that yields a group is reached
/// only through those meters, so the seed and its siblings together are
/// exactly the parents' children — the same set for every such seed. A
/// seed with an extra non-meter feed yields no group, and as a parents'
/// child itself it also fails the sibling checks of every other seed under
/// the same meters: all of them agree on `None`. Without the cache, a
/// rejected candidate would be rebuilt from every remaining target under
/// the same parents, walking the same sibling subtrees each time.
fn classify<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    seed: u64,
    targets: &BTreeSet<u64>,
    cache: &mut BTreeMap<BTreeSet<u64>, Option<Group>>,
) -> Result<Option<Group>, Error> {
    let Some(meters) = parent_meters(graph, seed)? else {
        return Ok(None);
    };
    if let Some(group) = cache.get(&meters) {
        return Ok(group.clone());
    }
    let group = classify_for_parents(graph, seed, targets, &meters)?;
    cache.insert(meters, group.clone());
    Ok(group)
}

/// [`classify`] after the parent meters are known: partitions the parents'
/// children into covered targets and subtracted siblings and builds the
/// [`Group`], or `None`.
fn classify_for_parents<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    seed: u64,
    targets: &BTreeSet<u64>,
    meters: &BTreeSet<u64>,
) -> Result<Option<Group>, Error> {
    // Partition the siblings into covered targets and subtracted others.
    // `siblings_from_predecessors` already excludes `seed` itself and dedups.
    let mut covered = Vec::new();
    let mut covered_has_meter = false;
    let mut subtracted = Vec::new();
    for sibling in graph.siblings_from_predecessors(seed)? {
        let sibling_id = sibling.component_id();
        if targets.contains(&sibling_id) {
            covered_has_meter |= sibling.is_meter();
            covered.push(sibling_id);
        } else if sibling.is_meter() || is_measurable_component(sibling, &graph.config) {
            subtracted.push(sibling_id);
        } else {
            // A sibling with no usable reading: its share of the parents'
            // readings is unknown.
            return Ok(None);
        }
    }
    // Non-target siblings make the group a subtraction. Decided here: the
    // pruning below can empty the list without changing the shape.
    let is_subtraction = !subtracted.is_empty();
    if !is_subtraction {
        // A covered sibling that is itself one of the parent meters passes
        // the feed check below against itself, but its flow already runs
        // through the other parents' readings. Accepting it would count that
        // line twice: once as a parallel meter and once as a covered
        // component.
        if covered.iter().any(|sibling| meters.contains(sibling)) {
            return Ok(None);
        }
    } else {
        // A target meter sibling: the parents' readings can't be split
        // between it and the other targets. (With no subtracted siblings a
        // covered meter is fine — the whole group merges into the parents.)
        if covered_has_meter {
            return Ok(None);
        }
        // The subtracted siblings must measure only what flows through the
        // parent meters, and must not lead to other targets (those are
        // measured on their own, so subtracting them here would drop them
        // from the total). A sibling that is itself one of the parent meters
        // is rejected here too: the seed below it is always a nested target.
        for &other in &subtracted {
            if !reached_only_through(graph, other, meters)? {
                return Ok(None);
            }
            if reaches_any_below(graph, other, targets)? {
                return Ok(None);
            }
        }
        // A subtracted sibling can feed another subtracted sibling — e.g. a
        // sub-meter next to the inverter it feeds, both under the parent
        // meter. The feeder's reading measures flow that is already inside
        // the fed sibling's own reading, so subtracting both would subtract
        // that flow twice. If all of a feeder's children are subtracted too,
        // they fully cover its reading, and the feeder is dropped from the
        // difference. Any other overlap can't be split cleanly, so no group
        // forms.
        let subtracted_set: BTreeSet<u64> = subtracted.iter().copied().collect();
        let mut redundant = Vec::new();
        for &other in &subtracted {
            if !reaches_any_below(graph, other, &subtracted_set)? {
                continue;
            }
            if graph.has_successors(other)?
                && graph
                    .successors(other)?
                    .all(|child| subtracted_set.contains(&child.component_id()))
            {
                redundant.push(other);
            } else {
                return Ok(None);
            }
        }
        subtracted.retain(|other| !redundant.contains(other));
        subtracted.sort_unstable();
    }
    // The seed and every covered target must be reached only through the
    // parent meters, so the meter readings cover the group's full
    // throughput — the same guard the subtracted siblings pass above.
    if !group_reached_only_through(graph, seed, &covered, meters)? {
        return Ok(None);
    }
    Ok(Some(Group {
        meters: meters.iter().copied().collect(),
        components: group_components(seed, covered),
        subtracted: is_subtraction.then_some(subtracted),
    }))
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
