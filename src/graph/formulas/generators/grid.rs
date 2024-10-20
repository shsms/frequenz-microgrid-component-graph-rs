// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid formulas.

use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct GridFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> GridFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self { graph })
    }

    /// Generates the grid formula for the given node.
    ///
    /// The grid formula is the sum of all components connected to the grid.
    /// This formula can be used for calculating power or current metrics at the
    /// grid connection point.
    pub fn build(self) -> Result<String, Error> {
        let mut expr = None;
        for comp in self.graph.successors(self.graph.root_id)? {
            let comp = self.graph.fallback_expr([comp.component_id()], true)?;
            expr = match expr {
                None => Some(comp),
                Some(e) => Some(comp + e),
            };
        }
        Ok(expr
            .map(|e| e.to_string())
            .unwrap_or_else(|| "0.0".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_utils::ComponentGraphBuilder;

    #[test]
    fn test_grid_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter and a battery chain behind it.
        let grid_meter = builder.meter();
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, meter_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?;
        assert_eq!(formula, "#1");

        // Add an additional dangling meter, and a PV chain and a battery chain
        // to the grid
        let dangling_meter = builder.meter();
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        let meter_pv_chain = builder.meter_pv_chain(1);
        builder.connect(grid, dangling_meter);
        builder.connect(grid, meter_bat_chain);
        builder.connect(grid, meter_pv_chain);

        assert_eq!(dangling_meter.component_id(), 5);
        assert_eq!(meter_bat_chain.component_id(), 6);
        assert_eq!(meter_pv_chain.component_id(), 9);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?;
        assert_eq!(formula, "#1 + #5 + COALESCE(#6, #7) + COALESCE(#9, #10)");

        // Add a PV inverter to the grid, without a meter.
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid, pv_inverter);

        assert_eq!(pv_inverter.component_id(), 11);

        let graph = builder.build(None)?;
        let formula = graph.grid_formula()?;
        assert_eq!(
            formula,
            "#1 + #5 + COALESCE(#6, #7) + COALESCE(#9, #10) + #11"
        );

        Ok(())
    }
}
