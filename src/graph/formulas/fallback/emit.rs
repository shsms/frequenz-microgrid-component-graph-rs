// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Builds the measurement expression for each resolved point.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::expr::Expr;
use crate::{ComponentGraph, Edge, Error, Node};

use super::SourcePreference;
use super::predicates::{is_grid_meter, reached_only_through, reaches_any_below};

/// The measurement expression for a single node.
pub(super) fn measure<N: Node, E: Edge>(
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
        //   so no trailing term is added. (Unreachable today: only
        //   components-first builds this ladder, components-first never
        //   allows meter chains, so a meter with a meter child stands alone
        //   instead of drilling here. Kept so the ladder stays right if that
        //   ever changes.)
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
pub(super) fn diamond_term(
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
pub(super) fn subtraction_term(
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

/// How one child contributes to its parent meter's children fallback sum.
/// The caller has already dropped children whose reading does not belong to
/// the meter's line alone (see the child filter in [`measure`]).
enum ChildTerm {
    /// A device child: its reading or 0 (`COALESCE(#id, 0)`), so the sum
    /// still resolves when the device is offline.
    ReadingOr0,
    /// A child meter whose children are also fed around it (a diamond below
    /// this level): its bare reading `#id`. A meter always measures its own
    /// feed line, so bare readings are safe to sum, but recursing into it
    /// would count the shared component once per feed.
    Bare,
    /// A child meter whose children are reached only through it: resolved
    /// recursively through [`measure`], so its own children back its
    /// reading. A bare `#id` would make the whole sum null while they still
    /// report. (A grid meter, and a meter without children, still come out
    /// of [`measure`] as a bare reading.)
    Recurse,
}

/// Decides a child's [`ChildTerm`] for its parent meter's children fallback
/// sum.
fn child_term_kind<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    child: &N,
) -> Result<ChildTerm, Error> {
    if !child.is_meter() {
        return Ok(ChildTerm::ReadingOr0);
    }
    let meter_id = child.component_id();
    let meters = BTreeSet::from([meter_id]);
    for successor in graph.successors(meter_id)? {
        if !reached_only_through(graph, successor.component_id(), &meters)? {
            return Ok(ChildTerm::Bare);
        }
    }
    Ok(ChildTerm::Recurse)
}

/// A child's contribution to a meter's `best` sum; one term per
/// [`ChildTerm`] decision.
fn child_best_effort_term<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    child: &N,
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let id = child.component_id();
    Ok(match child_term_kind(graph, child)? {
        ChildTerm::ReadingOr0 => Expr::coalesce(Expr::component(id), Expr::number(0.0)),
        ChildTerm::Bare => Expr::component(id),
        ChildTerm::Recurse => measure(graph, id, policy)?,
    })
}

/// Whether a meter's own reading is the only primary source (rather than
/// drilling into its children). Inside [`measure`], a stands-alone term is
/// still backed by the children's best-effort sum; only a grid meter stays
/// bare.
pub(super) fn stands_alone<N: Node, E: Edge>(
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

/// Sums the expressions, or `None` if there are none.
pub(super) fn sum(exprs: impl IntoIterator<Item = Expr>) -> Option<Expr> {
    exprs.into_iter().reduce(|a, b| a + b)
}
