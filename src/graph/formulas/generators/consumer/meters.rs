// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! The meters the no-grid-meter consumer path sums.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};

/// The topmost reporting meters that are not component meters — not a PV,
/// battery, EV charger, CHP, wind turbine or steam boiler meter. Their
/// readings sum to what the site draws through them: each covers one line,
/// from as high up as a reading exists.
pub(super) fn summed<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
) -> Result<BTreeSet<u64>, Error> {
    let found = graph.find_all(
        graph.root_id,
        |node| {
            node.is_meter()
                && !graph
                    .is_component_meter(node.component_id())
                    .unwrap_or(false)
                // A meter that provides no telemetry has no reading to sum;
                // not matching it makes discovery descend past it, so its
                // reporting descendant meters are summed instead.
                && node.provides_telemetry()
        },
        petgraph::Direction::Outgoing,
        false,
    )?;

    // Discovery descends past a no-telemetry meter, so on a parallel feed it
    // can collect a meter that a collected meter above it already measures.
    // Keep only the topmost collected meters: a covered line is then counted
    // once. What the covered meter gets through the silent feed alone goes
    // uncounted; no reading separates it.
    let mut kept = BTreeSet::new();
    for id in &found {
        let covered = graph.reaches_any(
            *id,
            |node| node.component_id() != *id && found.contains(&node.component_id()),
            petgraph::Direction::Incoming,
        )?;
        if !covered {
            kept.insert(*id);
        }
    }

    Ok(kept)
}
