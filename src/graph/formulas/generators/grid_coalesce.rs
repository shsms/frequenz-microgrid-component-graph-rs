// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid coalesce formulas.

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::Formula;
use crate::{ComponentGraph, Edge, Error, Node};

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
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn build(self) -> Result<Formula, Error> {
        let ids = self
            .graph
            .successors(self.graph.root_id)?
            .filter(|node| {
                node.is_meter()
                    || node.is_pv_inverter()
                    || node.is_battery_inverter(&self.graph.config)
            })
            .map(|node| node.component_id())
            .collect::<Vec<_>>();

        Ok(Formula::new(super::coalesce_with_telemetry(
            self.graph, ids,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_utils::graph;

    #[test]
    fn test_grid_voltage_formula() -> Result<(), Error> {
        // A grid meter and a battery chain behind it.
        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
            },
        }
        .build(None)?;
        let formula = graph.grid_coalesce_formula()?.to_string();
        assert_eq!(formula, "#1");

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
                // An additional dangling meter, and a PV chain and a battery
                // chain on the grid.
                5: meter,
                6: meter => { 7: inv_bat => { 8: bat } },
                9: meter => { 10: inv_pv },
            },
        }
        .build(None)?;
        let formula = graph.grid_coalesce_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#9, #6, #5, #1)");

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
                5: meter,
                6: meter => { 7: inv_bat => { 8: bat } },
                9: meter => { 10: inv_pv },
                // A PV inverter on the grid, without a meter.
                11: inv_pv,
            },
        }
        .build(None)?;
        let formula = graph.grid_coalesce_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#11, #9, #6, #5, #1)");

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_bat => { 4: bat } } },
                5: meter,
                6: meter => { 7: inv_bat => { 8: bat } },
                9: meter => { 10: inv_pv },
                11: inv_pv,
                // A PV inverter with no telemetry connected to the grid is
                // skipped.
                12: inv_pv [ControlOnly],
            },
        }
        .build(None)?;
        let formula = graph.grid_coalesce_formula()?.to_string();
        assert_eq!(formula, "COALESCE(#11, #9, #6, #5, #1)");

        Ok(())
    }
}
