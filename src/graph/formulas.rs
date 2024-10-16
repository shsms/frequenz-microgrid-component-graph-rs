// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

mod expr;
mod fallback;
mod generators;
mod traversal;

use crate::{ComponentGraph, Edge, Error, Node};

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    pub fn consumer_formula(&self) -> Result<String, Error> {
        generators::ConsumerFormulaBuilder::try_new(self)?.consumption_formula()
    }
}
