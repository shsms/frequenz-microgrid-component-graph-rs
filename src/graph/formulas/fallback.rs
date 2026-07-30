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

use crate::{ComponentGraph, Edge, Error, Node};

use super::expr::Expr;
use emit::{diamond_term, measure, subtraction_term, sum};
pub(super) use predicates::ids_with_telemetry;
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
        // Without fallback, each target is measured by its own reading; a target
        // that provides no telemetry has no reading to emit, so it is dropped.
        let mut terms: Vec<Expr> = ids_with_telemetry(graph, targets.iter().copied())?
            .into_iter()
            .map(Expr::component)
            .collect();
        // If every target was dropped for lack of telemetry, keep the term total
        // with a 0.0. A genuinely empty target set stays empty, as before.
        if terms.is_empty() && !targets.is_empty() {
            terms.push(Expr::number(0.0));
        }
        Ok(terms)
    } else {
        measurement_points(graph, &targets)?
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
    Ok(measure(graph, id, SourcePreference::MetersFirst)? == Expr::number(0.0))
}
