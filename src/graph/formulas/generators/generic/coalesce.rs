// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating `COALESCE` formulas for
//! measuring metrics from individual components, with fallback to other
//! components.

use std::collections::BTreeSet;

use crate::{graph::formulas::expr::Expr, ComponentGraph, Edge, Error, Node};

pub(crate) struct CoalesceFormulaBuilder {
    component_ids: BTreeSet<u64>,
}

impl CoalesceFormulaBuilder {
    pub fn try_new(
        graph: &ComponentGraph<impl Node, impl Edge>,
        component_ids: BTreeSet<u64>,
    ) -> Result<Self, Error> {
        if component_ids.is_empty() {
            return Err(Error::missing_parameters("No component IDs specified."));
        }
        for component_id in &component_ids {
            if graph.component(*component_id).is_err() {
                return Err(Error::component_not_found(format!(
                    "Component with ID {} not found in the graph.",
                    component_id
                )));
            }
        }
        Ok(Self { component_ids })
    }

    /// Generates a formula that uses the `COALESCE` function to return the first
    /// non-null value from the provided component IDs.
    pub fn build(self) -> Result<String, Error> {
        if self.component_ids.len() == 1 {
            if let Some(component_id) = self.component_ids.into_iter().next() {
                return Ok(Expr::Component { component_id }.to_string());
            } else {
                return Err(Error::internal(
                    "Failed to create expression for single component ID.",
                ));
            }
        }
        let expr = Expr::coalesce(
            self.component_ids
                .into_iter()
                .map(|component_id| Expr::Component { component_id })
                .collect(),
        );
        Ok(expr.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_utils::ComponentGraphBuilder;

    #[test]
    fn test_coalesce_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter and a battery chain behind it.
        let grid_meter_1 = builder.meter();
        builder.connect(grid, grid_meter_1);
        let grid_meter_2 = builder.meter();
        builder.connect(grid, grid_meter_2);

        let graph = builder.build(None)?;
        let formula = graph.coalesce(BTreeSet::from([1, 2]))?;
        assert_eq!(formula, "COALESCE(#1, #2)");
        let formula = graph.coalesce(BTreeSet::from([1]))?;
        assert_eq!(formula, "#1");
        let formula = graph.coalesce(BTreeSet::from([]));
        assert_eq!(
            formula,
            Err(Error::missing_parameters("No component IDs specified."))
        );

        Ok(())
    }
}
