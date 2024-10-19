// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Iterators over components and connections in a `ComponentGraph`.

use std::{collections::HashSet, iter::Flatten, vec::IntoIter};

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

/// An iterator over the siblings of a component in a `ComponentGraph`.
pub struct Siblings<'a, N>
where
    N: Node,
{
    pub(crate) component_id: u64,
    pub(crate) iter: Flatten<IntoIter<Neighbors<'a, N>>>,
    visited: HashSet<u64>,
}

impl<'a, N> Siblings<'a, N>
where
    N: Node,
{
    pub(crate) fn new(component_id: u64, iter: Flatten<IntoIter<Neighbors<'a, N>>>) -> Self {
        Siblings {
            component_id,
            iter,
            visited: HashSet::new(),
        }
    }
}

impl<'a, N> Iterator for Siblings<'a, N>
where
    N: Node,
{
    type Item = &'a N;

    fn next(&mut self) -> Option<Self::Item> {
        for i in self.iter.by_ref() {
            if i.component_id() == self.component_id || !self.visited.insert(i.component_id()) {
                continue;
            }
            return Some(i);
        }
        None
    }
}
