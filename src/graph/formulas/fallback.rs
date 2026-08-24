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
//! the group's own readings are the fallback (see
//! [`diamond_term`]).
//!
//! A meter's children can be a mix: some are targets, others are measured
//! nodes that are not targets. Those others can be sibling meters (e.g. a
//! PV+battery meter over PV inverters and a battery sub-meter) or non-target
//! components (e.g. one unreachable inverter next to its working siblings).
//! In that case the targets are measured as the parent meter minus those
//! siblings, with the component readings as the fallback (see
//! [`subtraction_term`]).
//!
//! # Resolution pipeline
//!
//! 1. `resolve`: [`measurement_points`] turns the target ids into ordered
//!    [`Measurement`] points — single nodes, diamonds, and subtractions —
//!    using one `classify` pass per seed.
//! 2. `emit`: one [`Expr`] per point, via [`measure`],
//!    [`diamond_term`], or
//!    [`subtraction_term`].
//! 3. [`aggregate`] sums the terms; [`aggregate_terms`] hands them to the
//!    caller unsummed.

mod emit;
mod predicates;
mod resolve;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};

use super::explain::{Explained, ExplanationKind, capitalized, sum_explained};
use super::expr::Expr;
pub(crate) use emit::diamond_term;
use emit::{measure, subtraction_term};
pub(crate) use predicates::{
    is_grid_meter, parent_meters, reached_only_through, reaches_any_below,
};
use resolve::{Measurement, measurement_points};

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
    MetersFirst {
        /// Whether a `prefer_meters_in_*` config option chose this (rather
        /// than the metric's own structure); explanations then say
        /// "preferred by config".
        by_config: bool,
    },
    /// Like [`SourcePreference::MetersFirst`], but a meter may also be measured through a
    /// single (non-component) child meter instead of standing alone.
    MetersFirstWithChains,
}

impl SourcePreference {
    /// Meter readings primary when `prefer_meters` (a config choice),
    /// component readings primary otherwise. (For meter chains, name
    /// [`SourcePreference::MetersFirstWithChains`].)
    pub(crate) fn prefer_meters(prefer_meters: bool) -> Self {
        if prefer_meters {
            SourcePreference::MetersFirst { by_config: true }
        } else {
            SourcePreference::ComponentsFirst
        }
    }

    /// Whether meter readings are the primary source (components the fallback).
    fn meters_first(self) -> bool {
        matches!(
            self,
            SourcePreference::MetersFirst { .. } | SourcePreference::MetersFirstWithChains
        )
    }

    /// Whether a `prefer_meters_in_*` config option is why meters are primary.
    fn meters_first_by_config(self) -> bool {
        matches!(self, SourcePreference::MetersFirst { by_config: true })
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
) -> Result<Explained, Error> {
    sum_explained(
        aggregate_terms(graph, targets, policy)?,
        ExplanationKind::TermSum,
        "One term per measurement point. The points do not overlap, so \
         summing them counts every component exactly once.",
    )
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
) -> Result<Vec<Explained>, Error> {
    aggregate_terms_avoiding(graph, targets, policy, &BTreeSet::new())
}

/// [`aggregate_terms`], with `off_limits` meters barred from standing in for
/// the groups they measure.
///
/// A caller that subtracts these terms from a sum of meter readings cannot use
/// a term that reads one of the summed meters: the subtraction would cancel
/// that meter out of the sum and take with it whatever else the meter reads,
/// the loads the graph does not model included. A group whose parent meters
/// include such a meter is measured node by node instead, exactly as a group
/// with no parent meter is — including the part that hurts: a node that
/// reports nothing has no reading to contribute and leaves its share of the
/// group unmeasured, where a parent meter would have covered it.
///
/// `off_limits` is honoured only while fallbacks are on. Without them every
/// target is measured by its own reading, which names a meter only if the
/// target is one; the current caller's targets and `off_limits` sets are
/// disjoint, so the question does not arise.
pub(crate) fn aggregate_terms_avoiding<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    targets: BTreeSet<u64>,
    policy: SourcePreference,
    off_limits: &BTreeSet<u64>,
) -> Result<Vec<Explained>, Error> {
    if graph.config.disable_fallback_components {
        // Without fallback, each target is measured by its own reading; a target
        // that provides no telemetry has no reading to emit, so it is dropped.
        // A silent part records each dropped target (its expression vanishes
        // from any sum); the silent parts follow the emitting terms.
        let mut terms: Vec<Explained> = Vec::new();
        let mut dropped: Vec<Explained> = Vec::new();
        for &id in &targets {
            let component = graph.component(id)?;
            if !component.provides_telemetry() {
                dropped.push(Explained::silent(
                    ExplanationKind::NoTelemetryZero,
                    format!(
                        "{} #{id} {} and provides no telemetry. It has no \
                         reading to emit, so it is dropped.",
                        capitalized(component.category().label()),
                        component.operational_mode().describe(),
                    ),
                    vec![id],
                ));
                continue;
            }
            let label = component.category().label();
            terms.push(
                Explained::leaf(
                    Expr::component(id),
                    ExplanationKind::FallbacksDisabled,
                    format!(
                        "Fallbacks are disabled by config, so {label} #{id} is \
                         measured by its bare reading only."
                    ),
                )
                .each(format!(
                    "Fallbacks are disabled by config, so each of the {{n}} \
                     {label}s is measured by its bare reading only."
                )),
            );
        }
        // If every target was dropped for lack of telemetry, keep the term total
        // with a 0.0. A genuinely empty target set stays empty, as before.
        if terms.is_empty() && !targets.is_empty() {
            terms.push(Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "Every target was dropped for lack of telemetry; 0.0 keeps the \
                 term total.",
            ));
        }
        terms.extend(dropped);
        Ok(terms)
    } else {
        measurement_points(graph, &targets, off_limits)?
            .into_iter()
            .map(|point| match point {
                Measurement::Single(id) => measure(graph, id, policy),
                Measurement::Diamond { components, meters } => {
                    diamond_term(graph, &components, &meters, policy)
                }
                Measurement::Subtraction {
                    parent_meters,
                    subtracted,
                    components,
                } => subtraction_term(graph, &parent_meters, &subtracted, &components, policy),
            })
            .collect()
    }
}

/// Whether `id`'s own measurement term is a plain `0.0`: neither it nor
/// anything the term can fall back to reports. Such a term subtracts
/// nothing. Parent meters standing in for `id` are not considered; that is
/// [`parent_meters`]' question.
pub(crate) fn measures_nothing<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
) -> Result<bool, Error> {
    Ok(measure(
        graph,
        id,
        SourcePreference::MetersFirst { by_config: false },
    )?
    .expr
        == Expr::number(0.0))
}
