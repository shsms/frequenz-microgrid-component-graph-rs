// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! The non-consumer chains a consumer measurement has to give back.
//!
//! Without phantom loads, the consumer formula starts from meters that read
//! whole lines. A battery, PV, CHP, EV charger, wind turbine or steam boiler
//! chain on such a line is inside that reading and is not consumption, so it
//! is subtracted back out.

use std::collections::BTreeSet;

use crate::{
    ComponentGraph, Edge, Error, Node,
    graph::formulas::fallback::{
        measures_nothing, parent_meters, reached_only_through, reaches_any_below,
    },
};

/// The chains to subtract from a consumer measurement, each measured by
/// exactly one target, so a caller subtracting their terms takes each chain
/// out once.
///
/// `summed` names the meters the measurement adds up — the reporting grid
/// meters, or the topmost reporting meters. A sum only holds what flows
/// through its own meters, so a chain fed from elsewhere as well stays
/// counted; see [`covered`].
pub(super) fn subtraction_targets<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    summed: &BTreeSet<u64>,
) -> Result<BTreeSet<u64>, Error> {
    let mut targets = covered(graph, component_chains_below(graph, graph.root_id)?, summed)?;

    // The search takes the topmost chain on each path, which leaves one
    // overlap: a chain fed from two places is reached on the path its own
    // meter is not on, so it comes back next to the meter above it. Only one
    // of the two may be subtracted, and it is the chain: what that meter
    // reads is one feed's worth, not a whole chain's.
    let mut kept = BTreeSet::new();
    while let Some(enclosing) = enclosing_target(graph, &targets, &kept)? {
        let replacements = covered(graph, component_chains_below(graph, enclosing)?, summed)?;
        if any_readable(graph, &replacements, summed)? {
            targets.remove(&enclosing);
            targets.extend(replacements);
        } else {
            // No chain below has a reading, so replacing the meter would
            // subtract nothing. Keep the meter: its reading covers the one
            // feed it carries, and a chain with no reading has no term
            // that could count the same flow again. A meter that reports
            // nothing itself is dropped instead — nothing on this chain
            // can be subtracted at all.
            if measures_nothing(graph, enclosing)? {
                targets.remove(&enclosing);
            } else {
                kept.insert(enclosing);
            }
            for chain in replacements {
                targets.remove(&chain);
            }
        }
    }

    Ok(targets)
}

/// The chains a measurement over `summed` holds in full. A chain reached only
/// through those meters is inside their readings; one with another feed — a
/// meter outside the set, or the grid itself — carries power the sum never
/// added, so taking its reading out would remove more than the sum holds.
/// The same check leaves out a chain hanging somewhere else entirely, such as
/// an inverter straight off the grid.
fn covered<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    chains: BTreeSet<u64>,
    summed: &BTreeSet<u64>,
) -> Result<BTreeSet<u64>, Error> {
    let mut held = BTreeSet::new();
    for id in chains {
        if reached_only_through(graph, id, summed)? {
            held.insert(id);
        }
    }
    Ok(held)
}

/// The topmost component chain on each path below `id`, `id` itself excluded:
/// a battery, PV, CHP, EV charger, wind turbine or steam boiler chain.
fn component_chains_below<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    id: u64,
) -> Result<BTreeSet<u64>, Error> {
    let mut chains = BTreeSet::new();
    for successor in graph.successors(id)? {
        chains.extend(graph.find_all(
            successor.component_id(),
            |node| {
                graph
                    .is_component_chain(node.component_id())
                    .unwrap_or(false)
            },
            petgraph::Direction::Outgoing,
            false,
        )?);
    }
    Ok(chains)
}

/// A target that another target sits below, if there is one. Targets in
/// `kept` were already decided to stay, so they are not offered again.
fn enclosing_target<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    targets: &BTreeSet<u64>,
    kept: &BTreeSet<u64>,
) -> Result<Option<u64>, Error> {
    for &id in targets {
        if !kept.contains(&id) && reaches_any_below(graph, id, targets)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Whether any chain can produce a reading to subtract: parent meters
/// standing in for it, or a measurement term of its own that does not
/// collapse to a plain 0.0 (see [`measures_nothing`]). A summed meter
/// cannot stand in: its reading is already spoken for in the caller's sum.
fn any_readable<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
    chains: &BTreeSet<u64>,
    summed: &BTreeSet<u64>,
) -> Result<bool, Error> {
    for &chain in chains {
        let stands_in =
            parent_meters(graph, chain)?.is_some_and(|meters| meters.is_disjoint(summed));
        if stands_in || !measures_nothing(graph, chain)? {
            return Ok(true);
        }
    }
    Ok(false)
}
