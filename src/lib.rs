// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

/*!
# Component Graph

`component_graph` is a library for representing the components of a microgrid
and the connections between them as a Directed Acyclic Graph (DAG).

A graph representation makes it easy to reason about the relationships between
the components and to come up with formulas for calculating aggregated metrics
for the microgrid.

## The `Node` and `Edge` traits

The main struct is [`ComponentGraph`], instances of which can be created by
passing an iterator of components and the connections between them to the
[`try_new`][ComponentGraph::try_new] method.

But because `component_graph` is an independent library, it doesn't know about
the component and connection types and instead uses traits to interact with
them.

Therefore, to be usable with this library, the component and connection types
must implement the [`Node`] and [`Edge`] traits, respectively.  Check out the
documentation for these traits for sample implementations.

## Validation

The [`try_new`][ComponentGraph::try_new] method several checks on the graph
including checking that:

- There is exactly one root node.
- All edges point to existing nodes.
- All nodes are reachable from the root node.
- There are no cycles in the graph.
- The components have sensible neighbor types.  For example, a battery shouldn't
  have successors and should have a battery inverter as a predecessor.

If any of the validation steps fail, the method will return an [`Error`], and a
[`ComponentGraph`] instance otherwise.
*/

mod component_category;
pub use component_category::{ComponentCategory, InverterType};

mod graph;
pub use graph::{iterators, ComponentGraph};

mod graph_traits;
pub use graph_traits::{Edge, Node};

mod error;
pub use error::Error;
