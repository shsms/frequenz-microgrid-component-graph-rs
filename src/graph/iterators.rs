// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Iterators over components and connections in a `ComponentGraph`.

use petgraph::graph::DiGraph;

use crate::{ComponentGraph, Edge, Node};

/// An iterator over the components in a `ComponentGraph`.
pub struct Components<'a, N>
where
    N: Node,
{
    pub(crate) iter: std::slice::Iter<'a, petgraph::graph::Node<N>>,
}

impl<'a, N> Iterator for Components<'a, N>
where
    N: Node,
{
    type Item = &'a N;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|n| &n.weight)
    }
}

/// An iterator over the connections in a `ComponentGraph`.
pub struct Connections<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub(crate) cg: &'a ComponentGraph<N, E>,
    pub(crate) iter: std::slice::Iter<'a, petgraph::graph::Edge<()>>,
}

impl<'a, N, E> Iterator for Connections<'a, N, E>
where
    N: Node,
    E: Edge,
{
    type Item = &'a E;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .and_then(|e| self.cg.edges.get(&(e.source(), e.target())))
    }
}

/// An iterator over the neighbors of a component in a `ComponentGraph`.
pub struct Neighbors<'a, N>
where
    N: Node,
{
    pub(crate) graph: &'a DiGraph<N, ()>,
    pub(crate) iter: petgraph::graph::Neighbors<'a, ()>,
}

impl<'a, N> Iterator for Neighbors<'a, N>
where
    N: Node,
{
    type Item = &'a N;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|i| &self.graph[i])
    }
}
