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
        generators::consumer::ConsumerFormulaBuilder::try_new(self)?.build()
    }

    pub fn grid_formula(&self) -> Result<String, Error> {
        generators::grid::GridFormulaBuilder::try_new(self)?.build()
    }

    pub fn producer_formula(&self) -> Result<String, Error> {
        generators::producer::ProducerFormulaBuilder::try_new(self)?.build()
    }
}
