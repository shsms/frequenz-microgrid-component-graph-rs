// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Generates the consumer formula: what the site's loads draw.
//!
//! There are three shapes, picked by configuration and then by topology:
//!
//! 1. With phantom loads configured in, every reporting meter contributes its
//!    residual, so power drawn by something the graph does not model still
//!    counts. See [`phantom_loads`].
//! 2. Otherwise, when every component directly under the grid is a grid meter,
//!    the grid reading measures the site. See
//!    [`ConsumerFormulaBuilder::build_with_grid_meter`].
//! 3. Otherwise, the topmost reporting meters ([`meters::summed`]) measure it
//!    between them. See
//!    [`ConsumerFormulaBuilder::build_without_grid_meter`].
//!
//! Shapes 2 and 3 both measure whole lines and then subtract the non-consumer
//! chains sitting on them (see [`chains`]); they differ in what they measure
//! from, and so in which chains that measurement covers.

mod chains;
mod meters;
mod phantom_loads;

#[cfg(test)]
mod tests;

use super::super::expr::Expr;
use crate::{
    ComponentGraph, Edge, Error, Node,
    graph::formulas::{
        Formula,
        fallback::{SourcePreference, aggregate_terms, is_grid_meter},
        generators::grid::GridFormulaBuilder,
    },
};

pub(crate) struct ConsumerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> ConsumerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self { graph })
    }

    /// Generates the consumer formula for the given node.
    pub fn build(self) -> Result<Formula, Error> {
        if self.graph.config.include_phantom_loads_in_consumer_formula {
            return phantom_loads::build(self.graph);
        }

        let grid_successors = self
            .graph
            .successors(self.graph.root_id)?
            .collect::<Vec<_>>();

        if grid_successors.is_empty() {
            return Ok(Formula::new(Expr::number(0.0)));
        }

        if grid_successors
            .iter()
            .all(|s| is_grid_meter(self.graph, s).unwrap_or(false))
        {
            self.build_with_grid_meter()
        } else {
            self.build_without_grid_meter()
        }
    }

    /// The grid reading, minus the component chains below it.
    ///
    /// The grid reading covers every feed the site has, so every chain in the
    /// graph is inside it and can be subtracted.
    fn build_with_grid_meter(&self) -> Result<Formula, Error> {
        let mut expr = GridFormulaBuilder::try_new(self.graph)?.build()?.expr;

        let targets = chains::component_chains(self.graph)?;
        for term in aggregate_terms(self.graph, targets, SourcePreference::MetersFirst)? {
            expr = expr - term;
        }

        Ok(Formula::new(expr.max(Expr::number(0.0))))
    }

    /// The sum of the topmost reporting meters.
    fn build_without_grid_meter(&self) -> Result<Formula, Error> {
        let summed = meters::summed(self.graph)?;

        let Some(expr) = summed
            .iter()
            .copied()
            .map(Expr::component)
            .reduce(|sum, component| sum + component)
        else {
            return Ok(Formula::new(Expr::number(0.0)));
        };

        Ok(Formula::new(expr.max(Expr::number(0.0))))
    }
}
