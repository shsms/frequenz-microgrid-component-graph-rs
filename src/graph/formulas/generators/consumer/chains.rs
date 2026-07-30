// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! The non-consumer chains a consumer measurement has to give back.
//!
//! Without phantom loads, the consumer formula starts from meters that read
//! whole lines. A battery, PV, CHP, EV charger, wind turbine or steam boiler
//! chain on such a line is inside that reading and is not consumption, so it
//! is subtracted back out. Each shape decides for itself which chains its own
//! measurement covers; this module only finds them.

use std::collections::BTreeSet;

use crate::{ComponentGraph, Edge, Error, Node};

/// The topmost component chain on each path down from the grid: a battery,
/// PV, CHP, EV charger, wind turbine or steam boiler chain. Search stops at
/// the first chain it meets on a path, so a chain below another is not
/// returned again by that path — a second feed into it does return it, and
/// both then stand as chains of their own.
pub(super) fn component_chains<N: Node, E: Edge>(
    graph: &ComponentGraph<N, E>,
) -> Result<BTreeSet<u64>, Error> {
    graph.find_all(
        graph.root_id,
        |node| {
            graph
                .is_component_chain(node.component_id())
                .unwrap_or(false)
        },
        petgraph::Direction::Outgoing,
        false,
    )
}
