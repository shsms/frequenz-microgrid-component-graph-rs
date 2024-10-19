// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for building formulas for various microgrid metrics.

use std::collections::BTreeSet;

use crate::ComponentGraph;
use crate::Edge;
use crate::Error;
use crate::Node;

mod expr;
mod fallback;
mod generators;
mod traversal;

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Returns a string representing the consumer formula for the graph.
    pub fn consumer_formula(&self) -> Result<String, Error> {
        generators::consumer::ConsumerFormulaBuilder::try_new(self)?.build()
    }

    /// Returns a string representing the grid formula for the graph.
    pub fn grid_formula(&self) -> Result<String, Error> {
        generators::grid::GridFormulaBuilder::try_new(self)?.build()
    }

    /// Returns a string representing the producer formula for the graph.
    pub fn producer_formula(&self) -> Result<String, Error> {
        generators::producer::ProducerFormulaBuilder::try_new(self)?.build()
    }

    /// Returns a string representing the battery formula for the graph.
    pub fn battery_formula(&self, battery_ids: Option<BTreeSet<u64>>) -> Result<String, Error> {
        generators::battery::BatteryFormulaBuilder::try_new(self, battery_ids)?.build()
    }

    /// Returns a string representing the CHP formula for the graph.
    pub fn chp_formula(&self, chp_ids: Option<BTreeSet<u64>>) -> Result<String, Error> {
        generators::chp::CHPFormulaBuilder::try_new(self, chp_ids)?.build()
    }
}
