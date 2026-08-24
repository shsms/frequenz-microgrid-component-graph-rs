// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Formula generators for standard metrics.

pub(super) mod battery;
pub(super) mod battery_ac_coalesce;
pub(super) mod category;
pub(super) mod consumer;
pub(super) mod grid;
pub(super) mod grid_coalesce;
pub(super) mod producer;
pub(super) mod pv_ac_coalesce;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::explain::{Explained, Explanation, ExplanationKind, capitalized};
use crate::graph::formulas::expr::Expr;
use crate::{ComponentGraph, Edge, Error, Node};

/// A `COALESCE` over the given component ids, in order, skipping any that
/// provide no telemetry — those have no reading to coalesce over, explained
/// as one [`ExplanationKind::CoalesceChain`] node with `rationale`. The
/// expression is [`Expr::None`] when nothing is left; each skipped source is
/// a silent part naming why it is not in the chain.
pub(super) fn coalesce_with_telemetry<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    ids: impl IntoIterator<Item = u64>,
    rationale: impl Into<String>,
) -> Result<Explained, Error> {
    let mut coalesced = Expr::None;
    let mut skipped = Vec::new();
    for id in ids {
        let component = graph.component(id)?;
        if component.provides_telemetry() {
            coalesced = coalesced.coalesce(Expr::component(id));
        } else {
            let label = match component.is_meter() {
                true => graph.meter_role_label(id)?,
                false => component.category().label(),
            };
            skipped.push(Explanation::silent(
                ExplanationKind::NoTelemetryZero,
                format!(
                    "{} #{id} {} and provides no telemetry, so it cannot \
                     serve as a source and is left out of the chain.",
                    capitalized(label),
                    component.operational_mode().describe(),
                ),
                vec![id],
            ));
        }
    }
    Ok(Explained::compose(
        coalesced,
        ExplanationKind::CoalesceChain,
        rationale,
        skipped,
    ))
}
