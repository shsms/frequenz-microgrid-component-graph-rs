// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Builds the measurement expression for each resolved point.
//!
//! Each function returns an [`Explained`]: the expression plus the tree of
//! reasons for its parts, captured from the branch that emitted them.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::expr::Expr;
use crate::{ComponentGraph, Edge, Error, Node};

use super::SourcePreference;
use super::predicates::{
    ids_with_telemetry, is_grid_meter, outside_feeds, reached_below, reached_only_through,
};
use crate::graph::formulas::explain::{
    Explained, Explanation, ExplanationKind, capitalized, id_list,
};

/// The measurement expression for a single node.
pub(super) fn measure<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
    policy: SourcePreference,
) -> Result<Explained, Error> {
    let own = Expr::component(id);
    let component = graph.component(id)?;
    if !component.is_meter() {
        // A component measured directly: its own reading, or 0. A component
        // that provides no telemetry has no reading to emit, so it
        // contributes 0.
        let label = component.category().label();
        if !component.provides_telemetry() {
            return Ok(Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::NoTelemetryZero,
                format!(
                    "{} #{id} {} and provides no telemetry: it has no \
                     reading, so it contributes 0.0.",
                    capitalized(label),
                    component.operational_mode().describe(),
                ),
            )
            .covering([id])
            .each(format!(
                "Each of the {{n}} {label}s provides no telemetry: it has no \
                 reading, so it contributes 0.0."
            )));
        }
        return Ok(Explained::leaf(
            own.coalesce(Expr::number(0.0)),
            ExplanationKind::DirectReading,
            format!(
                "{} #{id} is measured by its own reading. \
                 The 0.0 fallback keeps the term total when the reading is missing.",
                capitalized(label),
            ),
        )
        .each(format!(
            "Each of the {{n}} {label}s is measured by its own reading; the \
             0.0 fallback keeps each term total when its reading is missing."
        )));
    }
    // Asked once: several branches below turn on it, and it names the meter
    // in every reason they write. A grid meter is never a component meter,
    // so it has no role of its own to lose by being named for this one.
    let grid_meter = is_grid_meter(graph, component)?;
    let label = match grid_meter {
        true => "grid meter",
        false => graph.meter_role_label(id)?,
    };
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
    // The skipped children end up as silent parts on the meter's own node,
    // so they survive every branch below — also the ones that drop the
    // children sum.
    let mut skipped: Vec<Explanation> = Vec::new();
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
        } else {
            // Left out of every child sum, but not lost: the meter's own
            // reading still covers its flow, and a silent part records why
            // it adds no term of its own.
            let child_id = child.component_id();
            let covered = match component.provides_telemetry() {
                true => " The meter's own reading still covers its flow.",
                false => "",
            };
            skipped.push(Explanation::silent(
                ExplanationKind::NoTelemetryZero,
                format!(
                    "Child {} #{child_id} {} and provides no telemetry: it \
                     has no reading to add, so the child sums leave it \
                     out.{covered}",
                    child.category().label(),
                    child.operational_mode().describe(),
                ),
                vec![child_id],
            ));
        }
    }
    let contributing_ids: BTreeSet<u64> = contributing.iter().map(|c| c.component_id()).collect();
    let meters = BTreeSet::from([id]);
    let mut kept: Vec<&N> = Vec::new();
    for child in &contributing {
        let child_id = child.component_id();
        let child_label = child.category().label();
        let feeds = reached_below(graph, child_id, &contributing_ids)?;
        if !feeds.is_empty() {
            let (sibling, carrier) = match feeds.len() {
                1 => ("sibling", "its reading"),
                _ => ("siblings", "their readings"),
            };
            let rationale = format!(
                "Child {child_label} #{child_id} feeds {sibling} {} of the \
                 same parent; {carrier} already carr{} this child's flow, \
                 so even a bare reading would count that line twice. It is \
                 left out of the sum.",
                id_list(&feeds),
                match feeds.len() {
                    1 => "ies",
                    _ => "y",
                },
            );
            skipped.push(Explanation::silent(
                ExplanationKind::ChildSkipped {
                    feeds,
                    fed_from: vec![],
                },
                rationale,
                vec![child_id],
            ));
            continue;
        }
        let fed_from = outside_feeds(graph, child_id, &meters)?;
        if !fed_from.is_empty() {
            let rationale = format!(
                "Child {child_label} #{child_id} is also fed from outside \
                 meter #{id} (through {}), so its reading holds more than \
                 this meter passes. It is left out of the sum.",
                id_list(&fed_from),
            );
            skipped.push(Explanation::silent(
                ExplanationKind::ChildSkipped {
                    feeds: vec![],
                    fed_from,
                },
                rationale,
                vec![child_id],
            ));
        } else {
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
        let (summed, mut child_parts) = best_effort_children(graph, &kept, policy, true)?;
        if summed.is_none() && grid_meter {
            // No child read through it alone has a reading either, so there
            // is no reading at all: a 0.0 would assert one, so the term is
            // null.
            child_parts.extend(skipped);
            return Ok(Explained {
                expr: Expr::None,
                explanation: Explanation::silent_with_parts(
                    ExplanationKind::NoTelemetryZero,
                    format!(
                        "Grid meter #{id} {} and provides no telemetry, and no \
                         child read through it alone has a reading its term \
                         could use. It emits no term: a 0.0 would assert a \
                         reading that does not exist.",
                        component.operational_mode().describe(),
                    ),
                    vec![id],
                    child_parts,
                ),
            });
        }
        // An internal meter's term keeps the 0.0 its callers already expect.
        let Explained { expr, explanation } = match summed {
            Some(expr) => children_sum(expr, child_parts),
            None => Explained {
                expr: Expr::number(0.0),
                explanation: Explanation::new(
                    ExplanationKind::DefaultZero,
                    "No child adds a term of its own, so 0.0 keeps the term total.",
                    &Expr::number(0.0),
                    child_parts,
                ),
            },
        };
        let mut parts = vec![explanation];
        parts.extend(skipped);
        return Ok(Explained::compose(
            expr,
            ExplanationKind::NoTelemetryMeterDrill,
            format!(
                "{} #{id} {} and provides no telemetry, so its own reading \
                 must never appear. It is measured by the sum of its children \
                 instead.",
                capitalized(label),
                component.operational_mode().describe(),
            ),
            parts,
        ));
    }
    let meter_reading = |rationale: String| {
        Explained::leaf(
            Expr::component(id),
            ExplanationKind::MeterReading,
            rationale,
        )
    };
    // One binding for both branches below: the renderer treats equal
    // rationale text as one shared reason, printed once for a folded run.
    let only_source_reading = || {
        meter_reading(String::from(
            "The meter's own reading is the primary source: it is the \
             only source that covers its whole group.",
        ))
    };
    let standing = stands_alone(graph, id, policy)?;
    // A grid meter whose own reading is the only primary source stays bare:
    // it carries the site's unmodeled consumer load, which no sum of its
    // children accounts for. One that drills into a chain is a different
    // case — there the chain, not a child sum, backs the reading.
    if standing && grid_meter {
        return Ok(Explained::compose(
            own,
            ExplanationKind::BareGridMeter,
            format!(
                "Grid meter #{id} is measured by its bare reading. Its own \
                 reading is the only source here, and it also carries the \
                 site's unmodeled consumer load, which no sum of its \
                 children would account for, so nothing can back it."
            ),
            skipped,
        ));
    }
    if kept.is_empty() {
        // A childless meter stays bare, and so does one whose children were
        // all excluded to avoid a double count. When there are children but
        // none contribute, a 0.0 keeps the term total where their
        // best-effort sum otherwise would.
        if children.is_empty() {
            return Ok(Explained::leaf(
                own,
                ExplanationKind::BareMeter,
                format!(
                    "{} #{id} has no children, so it is measured by its bare reading.",
                    capitalized(label),
                ),
            ));
        }
        if contributing.is_empty() {
            // A grid meter stays bare either way: a 0.0 fallback would
            // assert a reading when its own is the only one there is.
            if grid_meter {
                return Ok(Explained::compose(
                    own,
                    ExplanationKind::BareGridMeter,
                    format!(
                        "Grid meter #{id} is measured by its bare reading. No \
                         child provides a usable reading, so a 0.0 fallback \
                         would assert one where its own is the only reading \
                         there is."
                    ),
                    skipped,
                ));
            }
            let reading = only_source_reading();
            let mut parts = vec![
                reading.explanation,
                Explanation::new(
                    ExplanationKind::DefaultZero,
                    "No child adds a term of its own, so 0.0 stands in for their sum.",
                    &Expr::number(0.0),
                    Vec::new(),
                ),
            ];
            parts.extend(skipped);
            return Ok(Explained::compose(
                own.coalesce(Expr::number(0.0)),
                ExplanationKind::MeterWithChildBackup,
                format!(
                    "{} #{id} is measured by its own reading. No child \
                     provides a usable reading, so a 0.0 keeps the term total \
                     where the children's best-effort sum otherwise would.",
                    capitalized(label),
                ),
                parts,
            ));
        }
        return Ok(Explained::compose(
            own,
            ExplanationKind::BareMeter,
            format!(
                "{} #{id} is measured by its bare reading: every child was \
                 left out of the fallback to avoid counting flow twice.",
                capitalized(label),
            ),
            skipped,
        ));
    }
    let empty = || Error::internal("Meter children sum is empty.");
    // The best-effort sum of the kept children: only they can back (or stand
    // in for) the meter reading; each kept child's reading-or-0 (child meters
    // resolve recursively, backed by their own children), so it resolves
    // whenever anything beneath it reports. A meter only drills when every
    // child provides telemetry (see [`stands_alone`]).
    let (summed, parts) = best_effort_children(graph, &kept, policy, false)?;
    let best = summed
        .map(|expr| children_sum(expr, parts))
        .ok_or_else(empty)?;
    if standing {
        // Standing alone means the meter's own reading is the only primary
        // source. It does not mean the term goes null when that reading is
        // missing: the children's sum still backs it — and keeps the term
        // total whenever every one of its own terms resolves.
        let reading = only_source_reading();
        let mut parts = vec![reading.explanation, best.explanation];
        parts.extend(skipped);
        let backup = match best.expr.always_resolves() {
            true => "the best-effort sum of its usable children keeps the term total.",
            false => {
                "the sum of its usable children backs it — though not every \
                 term there carries a 0.0 fallback, so the term can still go \
                 missing."
            }
        };
        return Ok(Explained::compose(
            own.coalesce(best.expr),
            ExplanationKind::MeterWithChildBackup,
            format!(
                "{} #{id} is measured by its own reading. If the reading \
                 goes missing, {backup}",
                capitalized(label),
            ),
            parts,
        ));
    }

    let kept_ids: Vec<u64> = kept.iter().map(|c| c.component_id()).collect();
    // The identity sentence opens the group's rationale and doubles as its
    // member reason: when a run of groups folds expanded, each member keeps
    // exactly this line while the shared prose prints once for the run.
    let identity = format!(
        "{} #{id} measures exactly this group ({}).",
        capitalized(label),
        id_list(&kept_ids),
    );
    let each = format!(
        "Each of the {{n}} {label} groups below is measured the same way, \
         from the same sources in this order; each term's own comment lists \
         its group."
    );
    if policy.meters_first() {
        let reading = meter_reading(String::from(
            "The meter's own reading is the primary source: it measures \
             all of its children together.",
        ));
        let mut parts = vec![reading.explanation, best.explanation];
        parts.extend(skipped);
        Ok(Explained::compose(
            own.coalesce(best.expr),
            ExplanationKind::MeterDrill {
                prefers_meters: true,
            },
            format!(
                "{identity} Meters are preferred{}, so its reading comes \
                 first; {} is the fallback.",
                match policy.meters_first_by_config() {
                    true => " by config",
                    false => "",
                },
                // One kept child is measured on its own — there is no sum
                // for it to be summed into.
                match kept.len() {
                    1 => "its child",
                    _ => "the sum of its children",
                },
            ),
            parts,
        )
        .each(each)
        .member(identity))
    } else {
        // `exact` is null unless every kept child reports.
        let exact =
            sum(kept.iter().map(|c| Expr::component(c.component_id()))).ok_or_else(empty)?;
        let exact = Explained::leaf(
            exact,
            ExplanationKind::ExactSum,
            "The children's own readings, summed exactly: null unless every \
             child reports, so a missing reading moves on to the fallback \
             instead of silently undercounting.",
        );
        let reading = meter_reading(String::from(
            "The group's meter measures the same components together, so \
             its reading can stand in when a child reading is missing.",
        ));
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
            Some(best)
        } else if kept[0].is_meter() {
            None
        } else {
            Some(Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "The last-resort 0.0 keeps the term total when both the child \
                 and the meter readings are missing.",
            ))
        };
        let mut expr = exact.expr.coalesce(reading.expr);
        let mut parts = vec![exact.explanation, reading.explanation];
        if let Some(last_resort) = last_resort {
            expr = expr.coalesce(last_resort.expr);
            parts.push(last_resort.explanation);
        }
        parts.extend(skipped);
        Ok(Explained::compose(
            expr,
            ExplanationKind::MeterDrill {
                prefers_meters: false,
            },
            format!(
                "{identity} Component readings are preferred, so their \
                 exact sum comes first; the meter reading is the fallback."
            ),
            parts,
        )
        .each(each)
        .member(identity))
    }
}

/// The children's best-effort terms, and their explanations in the same
/// order. The expression is `None` when no term is left to sum.
///
/// `drop_dead` leaves out a term that resolves to no reading — a plain 0.0 or
/// a null — recording it as a silent part instead, so a descent that finds no
/// readings falls through rather than summing fabricated zeros. Only a meter
/// measured *by* this sum needs that; one merely backed by it keeps the 0.0
/// its callers already expect.
fn best_effort_children<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    kept: &[&N],
    policy: SourcePreference,
    drop_dead: bool,
) -> Result<(Option<Expr>, Vec<Explanation>), Error> {
    // Decided here and nowhere else: this is the only place that knows how
    // many children the sum will hold, and it is the same count
    // `children_sum` collapses on. Deriving it per child, or passing it in
    // from a caller, is how the reasons drift away from the tree they
    // describe.
    let company = match kept.len() {
        1 => TermCompany::Alone,
        _ => TermCompany::WithSiblings,
    };
    let mut terms = Vec::new();
    let mut parts = Vec::new();
    for child in kept {
        let term = child_best_effort_term(graph, child, policy, company)?;
        if drop_dead && (matches!(term.expr, Expr::None) || term.expr == Expr::number(0.0)) {
            let child_id = child.component_id();
            parts.push(Explanation::silent_with_parts(
                ExplanationKind::NoTelemetryZero,
                format!(
                    "Child {} #{child_id} resolves to no reading of its \
                     own, so it backs nothing and the sum leaves it out.",
                    child.category().label(),
                ),
                vec![child_id],
                vec![term.explanation],
            ));
            continue;
        }
        terms.push(term.expr);
        parts.push(term.explanation);
    }
    Ok((sum(terms), parts))
}

/// The children's sum as one explained term. A sum of a single part is the
/// part itself: a node above it would carry the same expression and the same
/// components, saying only "the sum of this one thing". [`sum_explained`]
/// drops the extra node for the same reason.
fn children_sum(expr: Expr, parts: Vec<Explanation>) -> Explained {
    match <[Explanation; 1]>::try_from(parts) {
        Ok([only]) => Explained {
            expr,
            explanation: only,
        },
        Err(parts) => {
            let (kind, rationale) = children_sum_kind(&expr);
            Explained::compose(expr, kind, rationale, parts)
        }
    }
}

/// How to label a meter's children sum, which is best-effort only when every
/// term carries its own fallback. A child summed by its bare reading — one
/// fed around its meter, or a recursed meter with no fallback of its own —
/// has none, so the sum goes missing with that reading and must not claim
/// otherwise.
///
/// The kind and the rationale are chosen together, from one reading of the
/// expression, so the machine-readable label cannot promise what the prose
/// denies.
fn children_sum_kind(expr: &Expr) -> (ExplanationKind, &'static str) {
    match expr.always_resolves() {
        true => (
            ExplanationKind::BestEffortSum,
            "The best-effort sum of the meter's usable children: each reading \
             or 0.0, so it still resolves when only part of the group reports.",
        ),
        false => (
            ExplanationKind::ChildrenSum,
            "The sum of the meter's usable children. Not every term carries a \
             0.0 fallback of its own, so the sum goes missing when a bare \
             reading does.",
        ),
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
) -> Result<Explained, Error> {
    let empty = || Error::internal("Subtraction measurement with no components.");
    let no_meters = || Error::internal("Subtraction measurement with no parent meters.");
    if components.is_empty() {
        return Err(empty());
    }
    // Several parallel parent meters (a diamond) sum to the group's throughput;
    // a single meter is just that sum of one.
    let meter_sum = exact_sum(parent_meters).ok_or_else(no_meters)?;
    let difference = Explained::leaf(
        subtracted
            .iter()
            .fold(meter_sum, |expr, &m| expr - Expr::component(m)),
        ExplanationKind::MeterDifference {
            meters: parent_meters.to_vec(),
            subtracted: subtracted.to_vec(),
        },
        format!(
            "The parent meter(s) {} minus the sibling readings {}: everything \
             through the parents except what the siblings account for, which \
             is exactly this group. The siblings are subtracted by their bare \
             readings — a COALESCE(_, 0.0) there would attribute a missing \
             sibling's power to the group.",
            id_list(parent_meters),
            id_list(subtracted)
        ),
    );
    let rationale = format!(
        "Components {} share parent meter(s) {} with other children ({}), so \
         the group is measured as the parents minus those siblings.",
        id_list(components),
        id_list(parent_meters),
        id_list(subtracted)
    );
    // A component that provides no telemetry has no reading; the difference
    // still measures it, so it is dropped only from the component-side terms.
    // The exact component sum can be the primary source only when every
    // component reports; otherwise it undercounts the group. The difference
    // is then primary: it covers the whole group, including the missing
    // readings.
    let telemetry_components = ids_with_telemetry(graph, components.iter().copied())?;
    let all_report = telemetry_components.len() == components.len();
    let best = best_effort_sum(&telemetry_components).map(|expr| {
        Explained::leaf(
            expr,
            ExplanationKind::BestEffortSum,
            format!(
                "The best-effort sum of the components' readings ({}): each \
                 reading or 0.0, so the term always resolves.",
                id_list(&telemetry_components)
            ),
        )
    });
    if policy.meters_first() || !all_report {
        // When no component reports, the difference has no component-reading
        // backstop; fall back to 0.0 so the term stays total when the parent
        // meters are missing too.
        let backstop = best.unwrap_or_else(|| {
            Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "No component reports, so 0.0 keeps the term total when the \
                 parent meters are missing too.",
            )
        });
        Ok(Explained::compose(
            difference.expr.coalesce(backstop.expr),
            ExplanationKind::Subtraction,
            rationale,
            vec![difference.explanation, backstop.explanation],
        ))
    } else {
        let exact = Explained::leaf(
            exact_sum(&telemetry_components).ok_or_else(empty)?,
            ExplanationKind::ExactSum,
            "The components' own readings are the primary source. This sum is \
             null unless every component reports.",
        );
        let best = best.ok_or_else(empty)?;
        let last_resort = if components.len() > 1 {
            best
        } else {
            Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "The last-resort 0.0 keeps the term total when both the \
                 component and the parent meter readings are missing.",
            )
        };
        Ok(Explained::compose(
            exact
                .expr
                .coalesce(difference.expr)
                .coalesce(last_resort.expr),
            ExplanationKind::Subtraction,
            rationale,
            vec![
                exact.explanation,
                difference.explanation,
                last_resort.explanation,
            ],
        ))
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

/// Whether a child's term stands as the whole fallback or joins its siblings
/// in a sum. It decides only how the term's reason is worded — a lone term
/// must not describe a sum or a group that is not there, because
/// [`children_sum`] drops the sum node when there is only one part.
///
/// Set in exactly one place, [`best_effort_children`], from the same child
/// count [`children_sum`] collapses on. Keep it that way: a second site
/// deciding this is how a reason ends up describing a shape the tree does
/// not have.
#[derive(Clone, Copy)]
enum TermCompany {
    /// The only kept child. Its term is the fallback itself, with no sum
    /// around it.
    Alone,
    /// One of several kept children, summed together.
    WithSiblings,
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
///
/// `company` only picks the wording: every reason here must describe the
/// shape the term actually lands in, and a [`TermCompany::Alone`] term lands
/// as the fallback itself, with no sum and no group around it. The plural
/// phrasings set by `each` are exempt — the renderer prints one only for a
/// folded run of two or more, where a sum is always there to talk about.
fn child_best_effort_term<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    child: &N,
    policy: SourcePreference,
    company: TermCompany,
) -> Result<Explained, Error> {
    let id = child.component_id();
    Ok(match child_term_kind(graph, child)? {
        ChildTerm::ReadingOr0 => Explained::leaf(
            Expr::coalesce(Expr::component(id), Expr::number(0.0)),
            ExplanationKind::DirectReading,
            match company {
                TermCompany::Alone => format!(
                    "Child {} #{id}'s reading. The 0.0 fallback keeps the \
                     term total when the device is offline.",
                    child.category().label(),
                ),
                TermCompany::WithSiblings => format!(
                    "Child {} #{id}'s reading. The 0.0 fallback keeps the sum \
                     total when the device is offline, so one silent child \
                     does not null the whole group.",
                    child.category().label(),
                ),
            },
        )
        .each(format!(
            "Each of the {{n}} child {}s adds its reading, with a 0.0 \
             fallback so one offline device does not null the whole sum.",
            child.category().label(),
        )),
        ChildTerm::Bare => Explained::leaf(
            Expr::component(id),
            ExplanationKind::BareMeter,
            format!(
                "Child {} #{id}'s bare reading. Its children are also \
                 fed around it, so recursing into it would count the \
                 shared components once per feed; the bare reading is \
                 safe to {}.",
                graph.meter_role_label(id)?,
                match company {
                    TermCompany::Alone => "stand as the term",
                    TermCompany::WithSiblings => "sum",
                },
            ),
        ),
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
