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

use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::ids_with_telemetry;
use crate::{ComponentGraph, Edge, Error, Node};

/// A `COALESCE` over the given component ids, in order, skipping any that
/// provide no telemetry — those have no reading to coalesce over. Returns
/// [`Expr::None`] when nothing is left.
pub(super) fn coalesce_with_telemetry<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    ids: impl IntoIterator<Item = u64>,
) -> Result<Expr, Error> {
    Ok(ids_with_telemetry(graph, ids)?
        .into_iter()
        .fold(Expr::None, |coalesced, id| {
            coalesced.coalesce(Expr::component(id))
        }))
}
