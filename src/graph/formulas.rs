// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

mod consumption;
mod expressions;
mod grid;
mod traversal;

use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct FormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> FormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn new(graph: &'a ComponentGraph<N, E>) -> Self {
        Self { graph }
    }
}

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    pub fn consumer_formula(&self) -> Result<String, Error> {
        FormulaBuilder::new(self).consumption_formula()
    }
}
