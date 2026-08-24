// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Generates the consumer formula: what the site's loads draw.
//!
//! There are three shapes, picked by configuration and then by topology:
//!
//! 1. With phantom loads configured in, every reporting meter contributes its
//!    residual, so power drawn by something the graph does not model still
//!    counts. See [`phantom_loads`].
//! 2. Otherwise, when every component directly under the grid is a grid meter
//!    that reports, the grid reading measures the site. See
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

use std::collections::BTreeSet;

use super::super::expr::Expr;
use crate::component_category::CategoryPredicates;
use crate::{
    ComponentGraph, Edge, Error, Node,
    graph::formulas::{
        Formula,
        fallback::{SourcePreference, aggregate_terms, aggregate_terms_avoiding, is_grid_meter},
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
            .all(|s| is_grid_meter(self.graph, s).unwrap_or(false) && s.provides_telemetry())
        {
            self.build_with_grid_meter()
        } else {
            self.build_without_grid_meter()
        }
    }

    /// The grid reading, minus the component chains it covers.
    ///
    /// Every grid meter reports here — a graph with a silent one takes the
    /// summed-meters shape instead — so the grid reading covers every feed
    /// and every chain can be subtracted.
    fn build_with_grid_meter(&self) -> Result<Formula, Error> {
        let mut expr = GridFormulaBuilder::try_new(self.graph)?.build()?.expr;

        let meters = self
            .graph
            .successors(self.graph.root_id)?
            .map(|successor| successor.component_id())
            .collect::<BTreeSet<_>>();
        let targets = chains::subtraction_targets(self.graph, &meters)?;
        for term in aggregate_terms(
            self.graph,
            targets,
            SourcePreference::MetersFirst { by_config: false },
        )? {
            expr = expr - term;
        }

        Ok(Formula::new(expr.max(Expr::number(0.0))))
    }

    /// The sum of the topmost reporting meters, minus the component chains
    /// those readings cover.
    fn build_without_grid_meter(&self) -> Result<Formula, Error> {
        let summed = meters::summed(self.graph)?;

        let Some(mut expr) = summed
            .iter()
            .copied()
            .map(Expr::component)
            .reduce(|sum, component| sum + component)
        else {
            // Nothing to sum. A graph with no reading at all — the grid
            // formula is null too — has no answer; a graph that is merely
            // meterless consumes nothing it can see, and 0.0 is right.
            let grid = GridFormulaBuilder::try_new(self.graph)?.build()?.expr;
            return Ok(if matches!(grid, Expr::None) {
                Expr::None.into()
            } else {
                Formula::new(Expr::number(0.0))
            });
        };

        // A summed meter reads everything below it, non-consumer chains
        // included, so those have to come back out — the same subtraction
        // the grid-meter shape makes against the grid reading. The sum only
        // holds what flows through its own meters, so which chains it can
        // give back depends on those meters; `subtraction_targets` decides.
        let targets = chains::subtraction_targets(self.graph, &summed)?;

        // The summed meters are off limits as measurement sources: a term
        // reading one of them would cancel it out of the sum.
        for term in aggregate_terms_avoiding(
            self.graph,
            targets,
            SourcePreference::MetersFirst { by_config: false },
            &summed,
        )? {
            expr = expr - term;
        }

        Ok(Formula::new(expr.max(Expr::number(0.0))))
    }
}
