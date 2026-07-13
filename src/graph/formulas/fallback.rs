// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Resolves a set of target components into an aggregate measurement formula.
//!
//! A component (inverter, CHP, ...) is measured by its own reading, or — when an
//! upstream meter measures exactly that component and its in-target siblings — by
//! that meter instead (the substitution in [`measurement_points`]). A meter is
//! measured by a `COALESCE` of its own reading and the sum of its children,
//! ordered by the [`SourcePreference`]. Per-child fallbacks (`COALESCE(_, 0)`
//! for devices, recursion for child meters) back the reading, so the term
//! still resolves when the meter is offline but its children report. Two
//! kinds of meters stay bare, so their terms can go null: a grid meter (it
//! carries loads that are not in the graph), and one whose children's
//! readings do not belong to its line alone (a term for such a child would
//! count flow twice).
//!
//! When several parallel meters feed one component group (a diamond), their
//! readings measure distinct feed lines and so sum to the group's throughput;
//! the group's own readings are the fallback (see [`diamond_term`]).
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
use std::collections::{BTreeMap, BTreeSet};

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
    fn meters_first(self) -> bool {
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
                    diamond_term(&components, &meters, policy)
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
    /// [`diamond_term`].
    Diamond {
        components: Vec<u64>,
        meters: Vec<u64>,
    },
    /// A component group measured as its parent meter(s) minus their other
    /// children (sibling meters or non-target components); see [`subtraction_term`].
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
fn measurement_points<N: Node, E: Edge>(
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

/// Whether `id` reaches any member of `set` strictly below itself, following
/// the feed lines downward. The search starts at the node itself, so it is
/// excluded explicitly (`id` may be in `set`).
fn reaches_any_below<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    set: &BTreeSet<u64>,
) -> Result<bool, Error> {
    graph.reaches_any(
        id,
        |node| node.component_id() != id && set.contains(&node.component_id()),
        petgraph::Direction::Outgoing,
    )
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

/// The predecessor meters directly measuring `id`. `None` when `id` is not a
/// measurable component, has no parent meter, or is fed by a grid meter; then
/// neither the meter substitution nor the subtraction applies. A grid meter
/// (see [`is_grid_meter`]) carries the site's unmodeled consumer load, so its
/// reading covers more than its graph children: it can neither stand in for
/// them nor be split across them.
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
    for &meter in &meters {
        if is_grid_meter(graph, graph.component(meter)?)? {
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

/// The measurement expression for a single node.
fn measure<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let own = Expr::component(id);
    let component = graph.component(id)?;
    if !component.is_meter() {
        // A component measured directly: its own reading, or 0.
        return Ok(own.coalesce(Expr::number(0.0)));
    }
    let children: Vec<&N> = graph.successors(id)?.collect();
    let child_ids: BTreeSet<u64> = children.iter().map(|c| c.component_id()).collect();
    // A child backs the meter only when its reading belongs to this meter's
    // line alone. A child that feeds a sibling adds no term: the flow it
    // sends is already inside the fed sibling's reading, so a term for it
    // would count that flow twice. A child that is also fed from outside
    // this meter (a parallel meter's line) adds no term either: its reading
    // holds more than this meter passes. If an excluded child also carries
    // flow of its own, that share goes unseen — an accepted undercount in
    // that unusual wiring, and only in the fallback.
    let meters = BTreeSet::from([id]);
    let mut kept: Vec<&N> = Vec::new();
    for child in &children {
        let child_id = child.component_id();
        if !reaches_any_below(graph, child_id, &child_ids)?
            && reached_only_through(graph, child_id, &meters)?
        {
            kept.push(child);
        }
    }
    if kept.is_empty() {
        // Nothing sound to fall back to: the meter is measured bare.
        return Ok(own);
    }
    let standing = stands_alone(graph, id, policy)?;
    // A grid meter stays bare — it carries the site's unmodeled consumer
    // load, which no sum of its children accounts for.
    if standing && is_grid_meter(graph, component)? {
        return Ok(own);
    }
    let empty = || Error::internal("Meter children sum is empty.");
    // `best` sums each kept child's reading-or-0 (child meters resolve
    // recursively, backed by their own children), so it resolves whenever
    // anything beneath it reports.
    let terms = kept
        .iter()
        .map(|c| child_best_effort_term(graph, c, policy))
        .collect::<Result<Vec<_>, _>>()?;
    let best = sum(terms).ok_or_else(empty)?;
    if standing {
        // Standing alone means the meter's own reading is the only primary
        // source. It does not mean the term may go null when that reading is
        // missing: the children's best-effort sum still backs it, so the term
        // stays total.
        return Ok(own.coalesce(best));
    }

    if policy.meters_first() {
        Ok(own.coalesce(best))
    } else {
        // `exact` is null unless every kept child reports.
        let exact =
            sum(kept.iter().map(|c| Expr::component(c.component_id()))).ok_or_else(empty)?;
        // The last resort after `exact` and the meter:
        // - multiple kept children: `best`, the per-child reading-or-0 sum;
        // - a single kept device child: a plain 0 (`best` would just repeat
        //   the child already in `exact`);
        // - a single kept child meter: nothing — a meter is total on its own,
        //   so no trailing term is added.
        let last_resort = if kept.len() > 1 {
            best
        } else if kept[0].is_meter() {
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
fn diamond_term(
    components: &[u64],
    meters: &[u64],
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let empty = || Error::internal("Diamond measurement with no meters or components.");
    let component_sum = exact_sum(components).ok_or_else(empty)?;
    let meter_best = best_effort_sum(meters).ok_or_else(empty)?;
    Ok(if policy.meters_first() {
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
    Ok(if policy.meters_first() {
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

/// A child's contribution to a meter's `best` sum. The caller has already
/// dropped children whose reading does not belong to the meter's line alone
/// (see the child filter in [`measure`]).
///
/// A device child falls back to 0: `COALESCE(#id, 0)`.
///
/// A child meter whose children are reached only through it is resolved
/// recursively through [`measure`]. Its own children then back its reading; a
/// bare `#id` would make the whole sum null while they still report. Any
/// other child meter stays a bare `#id`. A meter always measures its own feed
/// line, so bare readings are safe to sum. But recursing into a child that is
/// also fed through a sibling (a diamond below this level) would count the
/// shared component once per feed. A grid meter also stays bare (inside
/// [`measure`]), and so does a meter without children — there is nothing
/// below it to fall back to.
fn child_best_effort_term<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    child: &N,
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let child_id = child.component_id();
    if !child.is_meter() {
        return Ok(Expr::coalesce(Expr::component(child_id), Expr::number(0.0)));
    }
    let meters = BTreeSet::from([child_id]);
    for successor in graph.successors(child_id)? {
        if !reached_only_through(graph, successor.component_id(), &meters)? {
            return Ok(Expr::component(child_id));
        }
    }
    measure(graph, child_id, policy)
}

/// Whether a meter's own reading is the only primary source (rather than
/// drilling into its children). Inside [`measure`], a stands-alone term is
/// still backed by the children's best-effort sum; only a grid meter stays
/// bare.
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
        // No child meters: drill in and sum them. A grid meter still stands
        // alone — it never falls back to its children (see [`measure`]).
        return is_grid_meter(graph, graph.component(id)?);
    }
    // Has a child meter: only drill through a single non-component child meter,
    // and only when meter chains are enabled.
    if !policy.allows_meter_chains() || successors.len() > 1 {
        return Ok(true);
    }
    graph.is_component_meter(successors[0].component_id())
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

    /// A stands-alone meter's fallback resolves child meters recursively, so
    /// the term still evaluates when the meter and its sub-meter are offline
    /// but the leaf components report.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 (PV),
    /// Meter:4 → Inverter:5 → Battery:6}, Meter:7}` — the grid meter (Meter:1)
    /// also feeds a load (Meter:7) so Meter:2 is an internal meter.
    #[test]
    fn test_stands_alone_total_through_child_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let battery_meter = builder.meter_bat_chain(1, 1);
        builder.connect(mixed_meter, battery_meter);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        let expr = aggregate(
            &graph,
            BTreeSet::from([mixed_meter.component_id()]),
            SourcePreference::MetersFirst,
        )?;
        // The battery sub-meter (#4) is backed by its inverter (#5), not a
        // bare `#4` that would null the sum while #5 still reports.
        assert_eq!(
            expr.to_string(),
            "COALESCE(#2, COALESCE(#4, #5, 0.0) + COALESCE(#3, 0.0))",
        );
        Ok(())
    }

    /// A meter substitution over an asymmetric diamond resolves to the same
    /// diamond regardless of which component id is lower. The seed fed by only
    /// one of the parallel meters is rejected (its sibling is also fed from
    /// outside that meter's parent set), and the sibling fed by the full meter
    /// set forms the diamond, subsuming the seed's standalone point.
    ///
    /// Topology: `Grid → GridMeter → {M_a, M_b}`, `M_a → {A, B}`, `M_b → B`,
    /// built once with A's id lower and once with B's.
    #[test]
    fn test_substitution_asymmetric_diamond_order_independent() -> Result<(), Error> {
        for shared_id_first in [false, true] {
            let mut builder = ComponentGraphBuilder::new();
            let grid = builder.grid();
            let grid_meter = builder.meter();
            builder.connect(grid, grid_meter);
            let m_a = builder.meter();
            let m_b = builder.meter();
            builder.connect(grid_meter, m_a);
            builder.connect(grid_meter, m_b);
            let first = builder.solar_inverter();
            let second = builder.solar_inverter();
            let (a, b) = if shared_id_first {
                (second, first)
            } else {
                (first, second)
            };
            builder.connect(m_a, a);
            builder.connect(m_a, b);
            builder.connect(m_b, b);

            let graph = builder.build(None)?;
            let targets = BTreeSet::from([a.component_id(), b.component_id()]);
            assert_eq!(
                super::measurement_points(&graph, &targets)?,
                vec![super::Measurement::Diamond {
                    components: vec![4, 5],
                    meters: vec![m_a.component_id(), m_b.component_id()],
                }],
                "shared_id_first: {shared_id_first}",
            );
            assert_eq!(
                aggregate(&graph, targets, SourcePreference::MetersFirst)?.to_string(),
                "COALESCE(#2 + #3, #4 + #5, COALESCE(#2, 0.0) + COALESCE(#3, 0.0))",
                "shared_id_first: {shared_id_first}",
            );
        }
        Ok(())
    }

    /// A meter substitution is rejected when a covered sibling is itself one
    /// of the seed's parent meters. Such a sibling passes the feed check
    /// against itself, but its flow already runs through the other parent's
    /// reading; a diamond built from this group would count that line twice —
    /// once as a parallel meter and once as a covered component. The group
    /// resolves through the seed whose parent set is just the outer meter:
    /// the nested feed passes `reached_only_through`, so the outer meter covers
    /// the whole group as a single point.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    /// Meter:4 → Inverter:5 → Battery:6}`, plus `Meter:2 → Inverter:5`
    /// directly, so Inverter:5's parents are {2, 4} and Meter:4 is both a
    /// parent meter and a sibling. Meter:1 also feeds a load (Meter:7).
    #[test]
    fn test_substitution_rejects_parent_meter_as_sibling() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let battery_meter = builder.meter();
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(mixed_meter, battery_meter);
        builder.connect(battery_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        builder.connect(mixed_meter, battery_inverter);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        // No diamond forms over {2, 4}: with Meter:4 also in the covered
        // components it would sum #2 + #4 while all of #4's flow is already
        // inside #2. Instead the seed under only Meter:2 covers the whole
        // group — Inverter:5's nested feed through Meter:4 still counts as
        // fed through Meter:2 — and the group is one point on that meter.
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([3, 4, 5]))?,
            vec![super::Measurement::Single(2)],
        );
        Ok(())
    }

    /// A child meter that shares a component with a sibling meter (a diamond
    /// one level below the meter being backed) stays a bare `#id` in the
    /// children fallback sum. Recursing into both siblings would resolve each
    /// to the shared component's reading and count it once per feed.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 (PV),
    /// Meter:4, Meter:5}, Meter:7}`, with both Meter:4 and Meter:5 feeding
    /// Inverter:6 (PV).
    #[test]
    fn test_child_meter_diamond_stays_bare() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv1 = builder.solar_inverter();
        builder.connect(mixed_meter, pv1);
        let m_a = builder.meter();
        let m_b = builder.meter();
        builder.connect(mixed_meter, m_a);
        builder.connect(mixed_meter, m_b);
        let pv2 = builder.solar_inverter();
        builder.connect(m_a, pv2);
        builder.connect(m_b, pv2);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        // Meters #4 and #5 stay bare — resolving both to their shared
        // inverter (#6) would subtract its reading twice when the meters are
        // offline. The sum of bare readings is safe: each meter measures its
        // own feed line.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([mixed_meter.component_id()]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "COALESCE(#2, #5 + #4 + COALESCE(#3, 0.0))",
        );
        Ok(())
    }

    /// A meter over a mix of target components and other meters measures the
    /// targets with the meter minus the sibling meters as the meter-side
    /// source, ordered against the component readings by the policy.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3..7 (PV),
    /// Meter:8}, Meter:9}` — a "PV + unspecified" meter next to an unspecified
    /// sub-meter, both under a grid meter (Meter:1) that also feeds a separate
    /// load (Meter:9), so Meter:2 is a genuine internal meter.
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
        // A second branch off the grid meter (a building load) keeps Meter:1 a
        // grid meter and Meter:2 an internal meter, not a sole-child fallback
        // grid meter that would carry the site's unmodeled load.
        let load_meter = builder.meter();
        builder.connect(main_meter, load_meter);

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

    /// Subtraction generalizes to a diamond: when a target's parents are several
    /// parallel meters, the group is measured as the summed meter readings minus
    /// the non-target siblings.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3} → {Inverter:4,
    /// Inverter:5 (PV)}` — both internal meters feed both inverters (a diamond).
    /// Meter:1 is the grid meter; Meter:2 and Meter:3 are internal.
    #[test]
    fn test_aggregate_subtraction_diamond() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let m_a = builder.meter();
        let m_b = builder.meter();
        builder.connect(grid_meter, m_a);
        builder.connect(grid_meter, m_b);
        let pv1 = builder.solar_inverter();
        let pv2 = builder.solar_inverter();
        for meter in [m_a, m_b] {
            builder.connect(meter, pv1);
            builder.connect(meter, pv2);
        }

        let graph = builder.build(None)?;
        let targets = BTreeSet::from([pv1.component_id()]);

        // One inverter behind the two parallel meters: the summed meter readings
        // minus the other inverter, with the inverter's own reading preferred.
        assert_eq!(
            super::measurement_points(&graph, &targets)?,
            vec![super::Measurement::Subtraction {
                parent_meters: vec![m_a.component_id(), m_b.component_id()],
                subtracted: vec![pv2.component_id()],
                components: vec![pv1.component_id()],
            }],
        );
        assert_eq!(
            aggregate(&graph, targets.clone(), SourcePreference::ComponentsFirst)?.to_string(),
            "COALESCE(#4, #2 + #3 - #5, 0.0)",
        );
        assert_eq!(
            aggregate(&graph, targets, SourcePreference::MetersFirst)?.to_string(),
            "COALESCE(#2 + #3 - #5, #4, 0.0)",
        );

        // With both inverters targeted there is nothing to subtract, so it stays
        // a pure diamond.
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([4, 5]))?,
            vec![super::Measurement::Diamond {
                components: vec![4, 5],
                meters: vec![2, 3],
            }],
        );
        Ok(())
    }

    #[test]
    fn test_subtraction_diamond_subsumes_earlier_single() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let m_a = builder.meter();
        let m_b = builder.meter();
        builder.connect(grid_meter, m_a);
        builder.connect(grid_meter, m_b);
        let pv1 = builder.solar_inverter();
        let bat_inverter = builder.battery_inverter();
        let battery = builder.battery();
        let pv2 = builder.solar_inverter();
        builder.connect(m_a, pv1);
        builder.connect(m_a, bat_inverter);
        builder.connect(bat_inverter, battery);
        builder.connect(m_a, pv2);
        builder.connect(m_b, pv2);

        let graph = builder.build(None)?;
        let targets = BTreeSet::from([pv1.component_id(), pv2.component_id()]);

        // Asymmetric diamond: only `pv2` is fed by both parallel meters, so the
        // seed `pv1` resolves to a standalone `Single` first (its own
        // subtraction sees only `m_a`'s parent set and is disqualified by
        // `pv2`'s feed from `m_b`). The group's subtraction then covers `pv1`
        // and must subsume that point, or its readings would be counted twice.
        assert_eq!(
            super::measurement_points(&graph, &targets)?,
            vec![super::Measurement::Subtraction {
                parent_meters: vec![m_a.component_id(), m_b.component_id()],
                subtracted: vec![bat_inverter.component_id()],
                components: vec![pv1.component_id(), pv2.component_id()],
            }],
        );
        assert_eq!(
            aggregate(&graph, targets, SourcePreference::ComponentsFirst)?.to_string(),
            "COALESCE(#4 + #7, #2 + #3 - #5, COALESCE(#4, 0.0) + COALESCE(#7, 0.0))",
        );
        Ok(())
    }

    /// A diamond whose parallel parent meters are grid meters — directly under
    /// the grid connection point, with mixed children so they are not component
    /// meters — carries the site's unmodeled load, so the subtraction is
    /// disqualified and the target falls back to its own reading, exercising the
    /// per-meter grid-meter guard on the diamond path.
    ///
    /// Topology (ids): `Grid:0 → {Meter:1, Meter:2} → {Inverter:3 (PV),
    /// Meter:4}`, both parallel meters feeding the shared inverter and sub-meter.
    #[test]
    fn test_subtraction_diamond_disqualified_by_grid_meters() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let m1 = builder.meter();
        let m2 = builder.meter();
        builder.connect(grid, m1);
        builder.connect(grid, m2);
        let pv = builder.solar_inverter();
        let sub_meter = builder.meter();
        for meter in [m1, m2] {
            builder.connect(meter, pv);
            builder.connect(meter, sub_meter);
        }

        let graph = builder.build(None)?;
        // The parallel parents are grid meters (mixed children, under the grid),
        // so the inverter falls back to its own reading rather than the
        // meter-sum-minus-sub-meter difference.
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
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
        // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 (PV),
        // Inverter:4 → Battery:5}, Meter:6}` — the grid meter (Meter:1) also
        // feeds a load (Meter:6) so Meter:2 is an internal meter.
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
        let load_meter = builder.meter();
        builder.connect(main_meter, load_meter);

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
        // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3,4 (PV),
        // Meter:5 → Inverter:6 → Battery:7}, Meter:8}` — the grid meter
        // (Meter:1) also feeds a load (Meter:8) so Meter:2 is an internal meter.
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
        let load_meter = builder.meter();
        builder.connect(main_meter, load_meter);

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
        // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:3 →
        // Battery:4, Meter:5 → Inverter:6 (PV)}, Meter:7}` — the grid meter
        // (Meter:1) also feeds a load (Meter:7) so Meter:2 is an internal meter.
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
        let load_meter = builder.meter();
        builder.connect(main_meter, load_meter);

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

        // Parent meter is a fallback grid meter: the sole child of the grid
        // meter, so it stands in for the grid connection point and likewise
        // carries the site's unmodeled consumer load.
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
        // Meter:4}`, where Meter:2 is Meter:1's only child.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        let fallback_grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, fallback_grid_meter);
        let pv = builder.solar_inverter();
        builder.connect(fallback_grid_meter, pv);
        let sub_meter = builder.meter();
        builder.connect(fallback_grid_meter, sub_meter);

        let graph = builder.build(None)?;
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");

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

        // Covered target also fed from outside the parent meter — here directly
        // by the grid, through no meter at all.
        // The extra inflow is not in the parent-meter reading, so the
        // meter-minus-siblings difference would undercount the group; the
        // targets are measured directly instead. Such a topology only arises
        // once neighbour validation is bypassed (the grid rule would otherwise
        // reject a second predecessor on a grid successor), so the guard is a
        // backstop for exactly that case.
        // Topology (ids): `Grid:0 → Meter:1 → {Meter:2 → {Inverter:4 (PV),
        // Inverter:5 (PV), Meter:6}, Meter:3}`, plus `Grid:0 → Inverter:5`.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let main_meter = builder.meter();
        let mixed_meter = builder.meter();
        let other_meter = builder.meter();
        builder.connect(grid, main_meter);
        builder.connect(main_meter, mixed_meter);
        builder.connect(main_meter, other_meter);
        let pv = builder.solar_inverter();
        let pv_external = builder.solar_inverter();
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, pv);
        builder.connect(mixed_meter, pv_external);
        builder.connect(mixed_meter, sub_meter);
        builder.connect(grid, pv_external);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_component_validation_failures(true)
                .build(),
        ))?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#4, 0.0) + COALESCE(#5, 0.0)",
        );
        Ok(())
    }

    /// A child meter that feeds a sibling of its own contributes no term to
    /// the children fallback sum: the flow it measures is already inside the
    /// fed sibling's reading, so a bare `#id` next to that sibling's term
    /// would count the shared line twice.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    /// Meter:4 → Inverter:5 → Battery:6}`, plus `Meter:2 → Inverter:5`
    /// directly, and a load (Meter:7) under the grid meter.
    #[test]
    fn test_children_fallback_drops_feeder_of_sibling() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let battery_meter = builder.meter();
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(mixed_meter, battery_meter);
        builder.connect(battery_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        builder.connect(mixed_meter, battery_inverter);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        // No bare `#4` next to `COALESCE(#5, 0.0)`: inverter #5's reading
        // already contains the flow through meter #4.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([mixed_meter.component_id()]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "COALESCE(#2, COALESCE(#5, 0.0) + COALESCE(#3, 0.0))",
        );
        Ok(())
    }

    /// A child that feeds a sibling through a nested meter is dropped from
    /// the children fallback too, not only a direct feeder: the flow through
    /// the nested chain is already inside the fed sibling's reading.
    #[test]
    fn test_children_fallback_drops_transitive_feeder() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let sub_a = builder.meter();
        builder.connect(mixed_meter, sub_a);
        let sub_b = builder.meter();
        builder.connect(sub_a, sub_b);
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(sub_b, battery_inverter);
        builder.connect(mixed_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        // No term for meter #3 next to the inverter's reading-or-0: inverter
        // #5's reading already contains the flow through meters #3 and #4.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([mixed_meter.component_id()]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "COALESCE(#2, #5, 0.0)",
        );
        Ok(())
    }

    /// A device child that feeds a sibling meter is dropped from the children
    /// fallback: the sibling meter's reading already contains its flow. The
    /// fed meter is dropped too — it is fed from outside the measured meter,
    /// so its reading holds more than the measured meter passes. Such edges
    /// exist only when validation failures are allowed.
    #[test]
    fn test_children_fallback_drops_device_feeder() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(mixed_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);
        builder.connect(battery_inverter, sub_meter);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_component_validation_failures(true)
                .build(),
        ))?;
        // No child term at all: inverter #3 feeds sibling #5, and meter #5
        // is fed by #3 from outside meter #2's line.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([mixed_meter.component_id()]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "#2",
        );
        Ok(())
    }

    /// A child fed by two parallel meters backs neither meter: its reading
    /// holds both meters' lines, so it would overstate each. Each meter is
    /// measured bare; a formula that targets the component itself measures
    /// the pair as a diamond instead.
    #[test]
    fn test_children_fallback_drops_child_shared_with_parallel_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let meter_a = builder.meter();
        builder.connect(grid_meter, meter_a);
        let meter_b = builder.meter();
        builder.connect(grid_meter, meter_b);
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(meter_a, battery_inverter);
        builder.connect(meter_b, battery_inverter);
        builder.connect(battery_inverter, battery);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        // No `COALESCE(#_, #4, 0.0)` terms: inverter #4's reading would be
        // subtracted once per parallel meter, counting its power twice.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([meter_a.component_id(), meter_b.component_id()]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "#2 + #3",
        );
        Ok(())
    }

    /// A sub-meter whose `Single` point was dropped for a covering group
    /// stays claimed: a later target below it must not substitute it back
    /// in — its flow is already inside the covering point.
    #[test]
    fn test_subsumed_sub_meter_stays_claimed() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let pv = builder.solar_inverter();
        builder.connect(grid_meter, pv);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let sub_meter = builder.meter();
        let chp = builder.chp();
        builder.connect(mixed_meter, sub_meter);
        builder.connect(mixed_meter, chp);
        let nested_chp = builder.chp();
        builder.connect(sub_meter, nested_chp);

        let graph = builder.build(None)?;
        // Seed order: sub-meter #4 first gets its own `Single`; CHP #5 then
        // substitutes mixed meter #3 in for the whole group and drops that
        // point. Nested CHP #6 must not claim #4 again — one term, not two.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([
                    sub_meter.component_id(),
                    chp.component_id(),
                    nested_chp.component_id(),
                ]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "COALESCE(#3, COALESCE(#5, 0.0) + COALESCE(#4, #6, 0.0))",
        );
        Ok(())
    }

    /// A grid meter is never backed by its children's sum, in the drilling
    /// path too: its reading carries the site's unmodeled consumer load,
    /// which no sum of its children accounts for. A fallback grid meter (the
    /// sole child of a grid meter) still backs the outer meter as a chain.
    #[test]
    fn test_grid_meter_never_backed_by_children() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let fallback_grid_meter = builder.meter();
        builder.connect(grid_meter, fallback_grid_meter);
        let pv = builder.solar_inverter();
        let chp = builder.chp();
        builder.connect(fallback_grid_meter, pv);
        builder.connect(fallback_grid_meter, chp);

        let graph = builder.build(None)?;
        // The chain falls back from #1 to #2, but never to the device sum:
        // the devices are only the producers, so their sum would report a
        // false value while both meters are offline.
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([grid_meter.component_id()]),
                SourcePreference::MetersFirstWithChains,
            )?
            .to_string(),
            "COALESCE(#1, #2)",
        );
        assert_eq!(
            aggregate(
                &graph,
                BTreeSet::from([fallback_grid_meter.component_id()]),
                SourcePreference::MetersFirst,
            )?
            .to_string(),
            "#2",
        );
        Ok(())
    }

    /// A subtracted sibling fed by another subtracted sibling is not counted
    /// twice. The sub-meter measures flow that is already inside the fed
    /// inverter's own reading. When the sub-meter feeds only subtracted
    /// siblings, it is dropped from the difference; when it also feeds
    /// something else, the difference can't be split and the subtraction
    /// does not apply.
    #[test]
    fn test_subtraction_covered_feeder() -> Result<(), Error> {
        // The sub-meter's only child is the battery inverter, which is also a
        // direct child of the mixed meter. Only the inverter is subtracted.
        // Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
        // Meter:4, Inverter:5 → Battery:6}`, plus `Meter:4 → Inverter:5`, and
        // a load (Meter:7) under the grid meter.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(mixed_meter, battery_inverter);
        builder.connect(sub_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#3, #2 - #5, 0.0)",
        );

        // The sub-meter also feeds a CHP of its own: dropping it would lose
        // the CHP's flow, keeping it would count the inverter's flow twice.
        // The subtraction does not apply.
        // Topology (ids): as above, plus `Meter:4 → CHP:7`; the load meter
        // is Meter:8.
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(mixed_meter, battery_inverter);
        builder.connect(sub_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        let chp = builder.chp();
        builder.connect(sub_meter, chp);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);

        let graph = builder.build(None)?;
        assert_eq!(graph.pv_formula(None)?.to_string(), "COALESCE(#3, 0.0)");
        Ok(())
    }

    /// A meter fed by both the grid connection point and another meter (a
    /// shape that only builds when validation failures are allowed) is a grid
    /// meter no matter in which order the edges were added. Every predecessor
    /// is checked, so the formula does not flip with edge order.
    ///
    /// Topology (ids): `Grid:0 → Meter:1`, `Meter:2 → {Inverter:3 (PV),
    /// Meter:4}`, with `Meter:2` fed by both `Grid:0` and `Meter:1`.
    #[test]
    fn test_grid_meter_second_feed_order_independent() -> Result<(), Error> {
        for grid_edge_first in [true, false] {
            let mut builder = ComponentGraphBuilder::new();
            let grid = builder.grid();
            let other_meter = builder.meter();
            builder.connect(grid, other_meter);
            let meter = builder.meter();
            if grid_edge_first {
                builder.connect(grid, meter);
                builder.connect(other_meter, meter);
            } else {
                builder.connect(other_meter, meter);
                builder.connect(grid, meter);
            }
            let pv = builder.solar_inverter();
            builder.connect(meter, pv);
            let sub_meter = builder.meter();
            builder.connect(meter, sub_meter);

            let graph = builder.build(Some(
                ComponentGraphConfig::builder()
                    .allow_component_validation_failures(true)
                    .build(),
            ))?;
            // The grid-fed meter is never a subtraction source.
            assert_eq!(
                graph.pv_formula(None)?.to_string(),
                "COALESCE(#3, 0.0)",
                "grid_edge_first: {grid_edge_first}",
            );
        }
        Ok(())
    }

    /// A cycle that is not reachable from the root survives validation when
    /// unconnected components are allowed (the acyclicity walk starts at the
    /// root). Formula generation must still stop on such a graph instead of
    /// recursing forever through the cycle's predecessors.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → Inverter:3 (PV)`, plus
    /// an off-root cycle `Meter:4 ↔ Meter:5` with `Meter:4 → Meter:2` and
    /// `Meter:4 → Inverter:3`.
    #[test]
    fn test_off_root_cycle_terminates() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let meter = builder.meter();
        builder.connect(grid_meter, meter);
        let pv = builder.solar_inverter();
        builder.connect(meter, pv);
        let x = builder.meter();
        let y = builder.meter();
        builder.connect(x, y);
        builder.connect(y, x);
        builder.connect(x, meter);
        builder.connect(x, pv);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_unconnected_components(true)
                .allow_component_validation_failures(true)
                .build(),
        ))?;
        // The exact formula does not matter on an invalid graph; generating
        // it just must terminate.
        graph.pv_formula(None)?;
        Ok(())
    }

    /// Pins that a subtraction stays a subtraction when the redundant-feeder
    /// pruning empties its subtracted set. Every subtracted sibling must then
    /// have all its successors inside the subtracted set, which needs a cycle
    /// among the siblings — so this input exists only on graphs kept alive by
    /// the validation allow-flags. The point still carries the subtraction
    /// shape (parent-meter sum, nothing subtracted), not a meter
    /// substitution.
    ///
    /// Topology (ids): `Grid:0 → Meter:1`, and an off-root `Meter:2 →
    /// {Inverter:3 (PV), Meter:4, Meter:5}` with `Meter:4 ↔ Meter:5`.
    #[test]
    fn test_pruned_empty_subtraction_keeps_shape() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let meter = builder.meter();
        let pv = builder.solar_inverter();
        builder.connect(meter, pv);
        let a = builder.meter();
        let b = builder.meter();
        builder.connect(meter, a);
        builder.connect(meter, b);
        builder.connect(a, b);
        builder.connect(b, a);

        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_unconnected_components(true)
                .allow_component_validation_failures(true)
                .build(),
        ))?;
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([3]))?,
            vec![super::Measurement::Subtraction {
                parent_meters: vec![2],
                subtracted: vec![],
                components: vec![3],
            }],
        );
        Ok(())
    }

    /// Pins the resolver's claim bookkeeping for meter targets.
    ///
    /// Only the consumer formula puts meters in the target set, and its
    /// target collection stops at the first component chain, so a target is
    /// never nested below another target meter. The scenarios here go beyond
    /// that caller contract on purpose: they pin how the claim set behaves
    /// today, so a restructuring that changes it is noticed. A deliberate
    /// behavior change may update these expectations.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Inverter:3 (PV),
    /// Inverter:4 (PV), Inverter:5 → Battery:6}`, and a load (Meter:7) under
    /// the grid meter.
    #[test]
    fn test_claim_semantics_meter_targets() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let pv1 = builder.solar_inverter();
        builder.connect(mixed_meter, pv1);
        let pv2 = builder.solar_inverter();
        builder.connect(mixed_meter, pv2);
        let battery_inverter = builder.inv_bat_chain(1);
        builder.connect(mixed_meter, battery_inverter);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);
        let graph = builder.build(None)?;

        // The meter target claims itself as a `Single`. The inverter's
        // subtraction is then rejected — its parent meter is already claimed
        // by a point that measures more than the inverter's group — and the
        // seed falls back to a standalone point.
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([2, 3]))?,
            vec![super::Measurement::Single(2), super::Measurement::Single(3)],
        );
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([2, 3, 4]))?,
            vec![
                super::Measurement::Single(2),
                super::Measurement::Single(3),
                super::Measurement::Single(4)
            ],
        );

        // With every child of the mixed meter targeted, the substitution
        // resolves the group onto the already-claimed meter: the group merges
        // into the existing point and emits nothing new.
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([2, 3, 4, 5]))?,
            vec![super::Measurement::Single(2)],
        );
        Ok(())
    }

    /// Pins that a subsumed point stays claimed: a later seed must not claim
    /// the same node again, because its flow is already inside the covering
    /// point. The input has a target nested below a target meter, which no
    /// current caller produces (see [`test_claim_semantics_meter_targets`] on
    /// the caller contract); the nested target then resolves to no point of
    /// its own.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Meter:3 → Inverter:5 →
    /// Battery:6, Inverter:4 (PV)}`, and a load (Meter:7) under the grid
    /// meter.
    #[test]
    fn test_subsumed_claim_stays_claimed() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let battery_inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(sub_meter, battery_inverter);
        builder.connect(battery_inverter, battery);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);
        let graph = builder.build(None)?;

        // Seed 3 emits Single(3); seed 4's substitution onto Meter:2 subsumes
        // it and drops that point. Meter:3 stays claimed, so seed 5's
        // substitution onto it emits nothing — its flow is already inside
        // Single(2).
        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([3, 4, 5]))?,
            vec![super::Measurement::Single(2)],
        );
        Ok(())
    }

    /// A target sibling that is a meter disqualifies the subtraction: the
    /// parent's reading can't be split between the meter target and the other
    /// targets. Both end up as standalone points.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → Meter:2 → {Meter:3, Inverter:4
    /// (PV), Inverter:5 → Battery:6}`, and a load (Meter:7) under the grid
    /// meter.
    #[test]
    fn test_subtraction_rejects_target_meter_sibling() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let mixed_meter = builder.meter();
        builder.connect(grid_meter, mixed_meter);
        let sub_meter = builder.meter();
        builder.connect(mixed_meter, sub_meter);
        let pv = builder.solar_inverter();
        builder.connect(mixed_meter, pv);
        let battery_inverter = builder.inv_bat_chain(1);
        builder.connect(mixed_meter, battery_inverter);
        let load_meter = builder.meter();
        builder.connect(grid_meter, load_meter);
        let graph = builder.build(None)?;

        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([3, 4]))?,
            vec![super::Measurement::Single(3), super::Measurement::Single(4)],
        );
        Ok(())
    }

    /// Two diamonds can never share a parallel meter: a shared meter means
    /// one group's components are also fed by a meter that feeds the other
    /// group, so the coverage check rejects both groups and each target stays
    /// a standalone point. This pins that the resolver's meter claims never
    /// meet an overlapping second diamond.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → {Meter:2, Meter:3, Meter:4}`, with
    /// `Meter:2 → Inverter:5`, `Meter:3 → {Inverter:5, Inverter:6}`,
    /// `Meter:4 → Inverter:6` (both PV).
    #[test]
    fn test_no_group_across_overlapping_diamonds() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);
        let m_a = builder.meter();
        let m_b = builder.meter();
        let m_c = builder.meter();
        builder.connect(grid_meter, m_a);
        builder.connect(grid_meter, m_b);
        builder.connect(grid_meter, m_c);
        let pv1 = builder.solar_inverter();
        let pv2 = builder.solar_inverter();
        builder.connect(m_a, pv1);
        builder.connect(m_b, pv1);
        builder.connect(m_b, pv2);
        builder.connect(m_c, pv2);
        let graph = builder.build(None)?;

        assert_eq!(
            super::measurement_points(&graph, &BTreeSet::from([5, 6]))?,
            vec![super::Measurement::Single(5), super::Measurement::Single(6)],
        );
        assert_eq!(
            graph.pv_formula(None)?.to_string(),
            "COALESCE(#5, 0.0) + COALESCE(#6, 0.0)",
        );
        Ok(())
    }

    /// The subtraction-diamond subsume resolves to the same point no matter
    /// which component id is lower — the twin of
    /// [`test_substitution_asymmetric_diamond_order_independent`] for the
    /// subtraction path. When the shared component's id is lower, the group
    /// forms directly on the first seed and there is no standalone point to
    /// subsume; the result is identical.
    ///
    /// Topology: `Grid → GridMeter → {M_a, M_b}`, `M_a → {PV_x, BatInverter →
    /// Battery, PV_shared}`, `M_b → PV_shared`, built once with PV_x's id
    /// lower and once with PV_shared's.
    #[test]
    fn test_subtraction_diamond_subsume_order_independent() -> Result<(), Error> {
        for shared_id_first in [false, true] {
            let mut builder = ComponentGraphBuilder::new();
            let grid = builder.grid();
            let grid_meter = builder.meter();
            builder.connect(grid, grid_meter);
            let m_a = builder.meter();
            let m_b = builder.meter();
            builder.connect(grid_meter, m_a);
            builder.connect(grid_meter, m_b);
            let first = builder.solar_inverter();
            let bat_inverter = builder.battery_inverter();
            let battery = builder.battery();
            let second = builder.solar_inverter();
            let (pv_x, pv_shared) = if shared_id_first {
                (second, first)
            } else {
                (first, second)
            };
            builder.connect(m_a, pv_x);
            builder.connect(m_a, bat_inverter);
            builder.connect(bat_inverter, battery);
            builder.connect(m_a, pv_shared);
            builder.connect(m_b, pv_shared);
            let graph = builder.build(None)?;

            let targets = BTreeSet::from([pv_x.component_id(), pv_shared.component_id()]);
            assert_eq!(
                super::measurement_points(&graph, &targets)?,
                vec![super::Measurement::Subtraction {
                    parent_meters: vec![m_a.component_id(), m_b.component_id()],
                    subtracted: vec![bat_inverter.component_id()],
                    components: {
                        let mut components = vec![pv_x.component_id(), pv_shared.component_id()];
                        components.sort_unstable();
                        components
                    },
                }],
                "shared_id_first: {shared_id_first}",
            );
        }
        Ok(())
    }
}
