// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Builds the measurement expression for each resolved point.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::expr::Expr;
use crate::{ComponentGraph, Edge, Error, Node};

use super::SourcePreference;
use super::predicates::{
    ids_with_telemetry, is_grid_meter, reached_only_through, reaches_any_below,
};
use crate::graph::formulas::explain::{Explained, ExplanationKind, id_list};

/// The measurement expression for a single node.
pub(super) fn measure<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let own = Expr::component(id);
    let component = graph.component(id)?;
    if !component.is_meter() {
        // A component measured directly: its own reading, or 0. A component
        // that provides no telemetry has no reading to emit, so it
        // contributes 0.
        if !component.provides_telemetry() {
            return Ok(Expr::number(0.0));
        }
        return Ok(own.coalesce(Expr::number(0.0)));
    }
    let children: Vec<&N> = graph.successors(id)?.collect();
    // A child that provides no telemetry has no reading to emit: it is
    // dropped from every child sum; the meter term itself covers it. A
    // silent child meter is the exception: when its children are reached
    // only through it and something below it reports, it contributes their
    // readings through recursion ([`ChildTerm::Recurse`]). A silent meter
    // whose children are fed around it stays out: its bare reading must
    // never be emitted, and recursing would count the shared components
    // once per feed. A reporting child backs the meter only when its
    // reading belongs to this meter's line alone. A child that feeds a
    // contributing sibling adds no term: the flow it sends is already
    // inside the fed sibling's reading (or, for a recursed silent meter,
    // its children's readings), so a term for it would count that flow
    // twice. (A fed sibling that contributes nothing has no term to double
    // count, so the feeder keeps its own term.) A child that is also fed
    // from outside this meter (a parallel meter's line) adds no term
    // either: its reading holds more than this meter passes. If an
    // excluded child also carries flow of its own, that share goes unseen
    // — an accepted undercount in that unusual wiring, and only in the
    // fallback.
    let mut contributing: Vec<&N> = Vec::new();
    for child in children.iter().copied() {
        if child.provides_telemetry()
            || (child.is_meter()
                && matches!(child_term_kind(graph, child)?, ChildTerm::Recurse)
                && graph.reaches_any(
                    child.component_id(),
                    |node| node.provides_telemetry(),
                    petgraph::Direction::Outgoing,
                )?)
        {
            contributing.push(child);
        }
    }
    let contributing_ids: BTreeSet<u64> = contributing.iter().map(|c| c.component_id()).collect();
    let meters = BTreeSet::from([id]);
    let mut kept: Vec<&N> = Vec::new();
    for child in &contributing {
        let child_id = child.component_id();
        if !reaches_any_below(graph, child_id, &contributing_ids)?
            && reached_only_through(graph, child_id, &meters)?
        {
            kept.push(child);
        }
    }
    if !component.provides_telemetry() {
        // A meter with no reading of its own: its `#id` must never be
        // emitted, so it is measured by the best-effort sum of its kept
        // children. A grid meter is no exception — the reporting meters
        // read through it alone cover its feed the same way the
        // consumer's summed meters do, missing unmodeled loads on the
        // silent segment and anything fed around it.
        // With nothing to descend to, a grid meter's term is null (there
        // is no reading at all, and a fabricated 0.0 would assert one);
        // an internal meter's keeps the 0.0 its callers already expect.
        let mut terms = kept
            .iter()
            .map(|c| child_best_effort_term(graph, c, policy))
            .collect::<Result<Vec<_>, _>>()?;
        // A child term that resolves to no reading — a plain 0.0 or a
        // null — backs nothing. It is dropped, so a descent that finds
        // no readings falls through instead of summing fabricated zeros.
        terms.retain(|term| !matches!(term, Expr::None) && *term != Expr::number(0.0));
        return Ok(sum(terms).unwrap_or_else(|| {
            if is_grid_meter(graph, component).unwrap_or(false) {
                Expr::None
            } else {
                Expr::number(0.0)
            }
        }));
    }
    let standing = stands_alone(graph, id, policy)?;
    // A grid meter stays bare — it carries the site's unmodeled consumer
    // load, which no sum of its children accounts for.
    if standing && is_grid_meter(graph, component)? {
        return Ok(own);
    }
    if kept.is_empty() {
        // A childless meter stays bare, and so does one whose children were
        // all excluded to avoid a double count. When there are children but
        // none contribute, a 0.0 keeps the term total where their
        // best-effort sum otherwise would.
        // A grid meter stays bare either way: a 0.0 fallback would
        // assert a reading when its own is the only one there is.
        return Ok(
            if !children.is_empty() && contributing.is_empty() && !is_grid_meter(graph, component)?
            {
                own.coalesce(Expr::number(0.0))
            } else {
                own
            },
        );
    }
    let empty = || Error::internal("Meter children sum is empty.");
    // `best` sums each kept child's reading-or-0 (child meters resolve
    // recursively, backed by their own children), so it resolves whenever
    // anything beneath it reports. A meter only drills when every child
    // provides telemetry (see [`stands_alone`]).
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
pub(crate) fn diamond_term<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    components: &[u64],
    meters: &[u64],
    policy: SourcePreference,
) -> Result<Explained, Error> {
    let empty = || Error::internal("Diamond measurement with no meters or components.");
    if components.is_empty() {
        return Err(empty());
    }
    // A meter that provides no telemetry never resolves: the exact meter
    // sum is dead with it in, and its best-effort term is a constant 0.
    // Both are left out of the formula — that is what the operational
    // mode is for. Only a fully reporting meter set keeps its exact sum.
    let reporting = ids_with_telemetry(graph, meters.iter().copied())?;
    let meters_report = reporting.len() == meters.len();
    let meter_best = best_effort_sum(&reporting).map(|expr| {
        Explained::leaf(
            expr,
            ExplanationKind::BestEffortSum,
            format!(
                "The best-effort sum of the reporting meters ({}): each \
                 reading or 0.0, so the term still resolves while any one of \
                 them reports.",
                id_list(&reporting)
            ),
        )
    });
    let meter_exact = || -> Result<Explained, Error> {
        Ok(Explained::leaf(
            exact_sum(meters).ok_or_else(empty)?,
            ExplanationKind::ExactSum,
            format!(
                "The exact sum of the meters ({}). Each meter measures a \
                 distinct feed line into the group, so their readings sum to \
                 the group's throughput. Null unless every meter reports.",
                id_list(meters)
            ),
        ))
    };
    let rationale = format!(
        "Components {} form one group fed through the parallel meters {}. \
         Each meter measures a distinct feed line, so the meter readings sum \
         to the group's throughput.",
        id_list(components),
        id_list(meters)
    );
    // A component that provides no telemetry has no reading; the meter
    // readings still measure it, so it is dropped only from the
    // component-side term. The component readings can stand in for the meter
    // sum only when every component reports; otherwise their sum would
    // undercount the group, so the meters are the sole source of the group
    // total.
    let all_report =
        ids_with_telemetry(graph, components.iter().copied())?.len() == components.len();
    if all_report {
        let component_sum = Explained::leaf(
            exact_sum(components).ok_or_else(empty)?,
            ExplanationKind::ExactSum,
            format!(
                "The exact sum of the components' own readings ({}). It can \
                 stand in for the meter sum because every component reports.",
                id_list(components)
            ),
        );
        Ok(match meter_best {
            Some(meter_best) if policy.meters_first() && meters_report => {
                let meter_sum = meter_exact()?;
                Explained::compose(
                    meter_sum
                        .expr
                        .coalesce(component_sum.expr)
                        .coalesce(meter_best.expr),
                    ExplanationKind::Diamond,
                    rationale,
                    vec![
                        meter_sum.explanation,
                        component_sum.explanation,
                        meter_best.explanation,
                    ],
                )
            }
            Some(meter_best) => Explained::compose(
                component_sum.expr.coalesce(meter_best.expr),
                ExplanationKind::Diamond,
                rationale,
                vec![component_sum.explanation, meter_best.explanation],
            ),
            None => Explained::compose(
                component_sum.expr,
                ExplanationKind::Diamond,
                format!(
                    "{rationale} No meter reports, so no meter-side term could \
                     ever resolve; the components' own readings are the whole \
                     term."
                ),
                vec![component_sum.explanation],
            ),
        })
    } else if meters_report {
        let meter_sum = meter_exact()?;
        let meter_best = meter_best.ok_or_else(empty)?;
        Ok(Explained::compose(
            meter_sum.expr.coalesce(meter_best.expr),
            ExplanationKind::Diamond,
            format!(
                "{rationale} Some components provide no telemetry, so the \
                 component readings would undercount the group; only the \
                 meters measure the group total."
            ),
            vec![meter_sum.explanation, meter_best.explanation],
        ))
    } else {
        // Neither a full meter sum nor a full component sum exists; the
        // reporting meters' best effort is the most the formula can say.
        Ok(match meter_best {
            Some(meter_best) => Explained::compose(
                meter_best.expr,
                ExplanationKind::Diamond,
                format!(
                    "{rationale} Some components and some meters provide no \
                     telemetry, so neither side has a sum that covers the \
                     group; the reporting meters' best effort is the most the \
                     formula can say."
                ),
                vec![meter_best.explanation],
            ),
            None => Explained::silent(
                ExplanationKind::NoTelemetryZero,
                format!(
                    "{rationale} Neither the meters nor every component \
                     reports, so nothing here can resolve: a term would \
                     assert a reading the group does not have."
                ),
                components.iter().chain(meters).copied().collect(),
            ),
        })
    }
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
pub(super) fn subtraction_term<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    parent_meters: &[u64],
    subtracted: &[u64],
    components: &[u64],
    policy: SourcePreference,
) -> Result<Expr, Error> {
    let empty = || Error::internal("Subtraction measurement with no components.");
    let no_meters = || Error::internal("Subtraction measurement with no parent meters.");
    if components.is_empty() {
        return Err(empty());
    }
    // Several parallel parent meters (a diamond) sum to the group's throughput;
    // a single meter is just that sum of one.
    let meter_sum = exact_sum(parent_meters).ok_or_else(no_meters)?;
    let difference = subtracted
        .iter()
        .fold(meter_sum, |expr, &m| expr - Expr::component(m));
    // A component that provides no telemetry has no reading; the difference
    // still measures it, so it is dropped only from the component-side terms.
    // The exact component sum can be the primary source only when every
    // component reports; otherwise it undercounts the group. The difference
    // is then primary: it covers the whole group, including the missing
    // readings.
    let telemetry_components = ids_with_telemetry(graph, components.iter().copied())?;
    let all_report = telemetry_components.len() == components.len();
    let best = best_effort_sum(&telemetry_components);
    if policy.meters_first() || !all_report {
        // When no component reports, the difference has no component-reading
        // backstop; fall back to 0.0 so the term stays total when the parent
        // meters are missing too.
        Ok(difference.coalesce(best.unwrap_or_else(|| Expr::number(0.0))))
    } else {
        let exact = exact_sum(&telemetry_components).ok_or_else(empty)?;
        let best = best.ok_or_else(empty)?;
        let last_resort = if components.len() > 1 {
            best
        } else {
            Expr::number(0.0)
        };
        Ok(exact.coalesce(difference).coalesce(last_resort))
    }
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
    if successors
        .iter()
        .any(|s| !s.provides_telemetry() && !s.is_meter())
    {
        // A device child that provides no telemetry has no reading to sum,
        // so no child sum is exact for the group: the meter's own reading
        // stays the primary source, backed by the children's best-effort
        // sum. A silent child meter is no such stop — it resolves through
        // its descendants — so a sole-child meter chain still drills; a
        // grid meter would otherwise go bare and lose the deeper
        // fallbacks its chain provides.
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
