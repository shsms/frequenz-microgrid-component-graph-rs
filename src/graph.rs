// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! A graph representation of the electrical components that are part of a
//! microgrid, and the connections between them.

mod creation;
mod meter_roles;
mod retrieval;
mod validation;

pub mod iterators;

use crate::{Edge, Node};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

/// `Node`s stored in a `DiGraph` instance can be addressed with `NodeIndex`es.
///
/// `NodeIndexMap` stores the corresponding `NodeIndex` for any `component_id`, so
/// that Nodes in the `DiGraph` can be retrieved from their `component_id`s.
pub(crate) type NodeIndexMap = HashMap<u64, NodeIndex>;

/// `Edge`s are not stored in the `DiGraph` instance, so we need to store them
/// separately.
///
/// `EdgeMap` can be used to lookup the `Edge` for any pair of source and
/// destination `NodeIndex` values.
pub(crate) type EdgeMap<E> = HashMap<(NodeIndex, NodeIndex), E>;

/// A graph representation of the electrical components of a microgrid and the
/// connections between them.
pub struct ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    graph: DiGraph<N, ()>,
    node_indices: NodeIndexMap,
    root_id: u64,
    edges: EdgeMap<E>,
}

#[cfg(test)]
mod test_utils;
