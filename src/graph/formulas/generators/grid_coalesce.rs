// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid coalesce formulas.

use crate::component_category::CategoryPredicates;
use crate::{
    graph::formulas::{expr::Expr, Formula},
    ComponentGraph, Edge, Error, Node,
};

pub(crate) struct GridCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> GridCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self { graph })
    }

    /// Generates the grid coalesce formula from the component graph.
    ///
    /// This formula is used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all meters, PV
    /// inverters, and battery inverters that are directly connected to the
    /// grid.
    pub fn build(self) -> Result<Formula, Error> {
        return Ok(Formula::new(Expr::coalesce(
            self.graph
                .successors(self.graph.root_id)?
                .filter(|node| {
                    node.is_meter()
                        || node.is_pv_inverter()
                        || node.is_battery_inverter(&self.graph.config)
                })
                .map(|comp| Expr::component(comp.component_id()))
                .collect(),
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_utils::ComponentGraphBuilder;

    #[test]
    fn test_grid_voltage_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter and a battery chain behind it.
        let grid_meter = builder.meter();
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, meter_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.grid_coalesce_formula()?.to_string();
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
        let formula = graph.grid_coalesce_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#9, #6, #5, #1)");

        // Add a PV inverter to the grid, without a meter.
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid, pv_inverter);

        assert_eq!(pv_inverter.component_id(), 11);

        let graph = builder.build(None)?;
        let formula = graph.grid_coalesce_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#11, #9, #6, #5, #1)");

        Ok(())
    }
}
