// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating consumer formulas.

use std::collections::{BTreeMap, BTreeSet};

use super::super::expr::Expr;
use crate::{
    component_category::CategoryPredicates, graph::formulas::AggregationFormula, ComponentGraph,
    Edge, Error, Node,
};

pub(crate) struct ConsumerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    unvisited_meters: BTreeSet<u64>,
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> ConsumerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self {
            unvisited_meters: graph.find_all(
                graph.root_id,
                |node| node.is_meter(),
                petgraph::Direction::Outgoing,
                true,
            )?,
            graph,
        })
    }

    /// Generates the consumer formula for the given node.
    pub fn build(mut self) -> Result<AggregationFormula, Error> {
        let mut all_meters = None;
        while let Some(meter_id) = self.unvisited_meters.pop_first() {
            let consumption = self.component_consumption(meter_id)?;
            if let Some(expr) = all_meters {
                all_meters = Some(expr + consumption);
            } else {
                all_meters = Some(consumption);
            }
        }

        let other_grid_successors = self
            .graph
            .successors(self.graph.root_id)?
            .filter(|s| !s.is_meter() && !s.is_battery_inverter(&self.graph.config))
            .map(|s| self.component_consumption(s.component_id()))
            .reduce(|a, b| Ok(a? + b?));

        let other_grid_successors = match other_grid_successors {
            Some(Ok(expr)) => Some(expr),
            Some(Err(err)) => return Err(err),
            None => None,
        };

        match (all_meters, other_grid_successors) {
            (Some(lhs), Some(rhs)) => Ok(AggregationFormula::new(lhs + rhs)),
            (None, Some(expr)) | (Some(expr), None) => Ok(AggregationFormula::new(expr)),
            (None, None) => Ok(AggregationFormula::new(Expr::number(0.0))),
        }
    }

    fn component_consumption(&mut self, component_id: u64) -> Result<Expr, Error> {
        let component = self.graph.component(component_id)?;
        if component.is_meter() {
            self.unvisited_meters.remove(&component_id);
            // Create a formula expression from the component.
            let mut expr = Expr::from(component);

            // If there are siblings with the same successors as the component,
            // then it is a diamond configuration, so we add those siblings to
            // the expression.
            let mut successors = BTreeMap::from_iter(
                self.graph
                    .successors(component_id)?
                    .map(|s| (s.component_id(), s)),
            );
            for sibling in self.graph.siblings_from_successors(component_id)? {
                expr = expr + sibling.into();
                self.unvisited_meters.remove(&sibling.component_id());
                for successor in self.graph.successors(sibling.component_id())? {
                    successors.insert(successor.component_id(), successor);
                }
            }

            // Subtract each successor from the expression.
            for successor in successors {
                let successor_expr = if successor.1.is_meter() {
                    self.graph.fallback_expr([successor.0], true)?
                } else {
                    Expr::from(successor.1)
                };
                expr = expr - successor_expr;
            }

            expr = expr.max(Expr::number(0.0));

            // If the meter doesn't have any meter successors, its consumption
            // can be 0 when it can't be calculated.
            if self.graph.has_successors(component_id)?
                && !self.graph.has_meter_successors(component_id)?
            {
                expr = expr.coalesce(Expr::number(0.0));
            }
            Ok(expr)
        } else {
            Ok(Expr::from(component).max(Expr::number(0.0)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graph::test_utils::ComponentGraphBuilder, ComponentGraphConfig};

    #[test]
    fn test_zero_consumers() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a battery inverter to the grid, without a battery meter.
        let inv_bat_chain = builder.inv_bat_chain(1);
        builder.connect(grid, inv_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(formula, "0.0");

        Ok(())
    }

    #[test]
    fn test_consumer_formula_with_grid_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter to the grid, with no successors.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(formula, "MAX(#1, 0.0)");

        // Add a battery meter with one battery inverter and one battery to the
        // grid meter.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        // Formula subtracts the battery meter from the grid meter, and the
        // battery inverter from the battery meter.
        assert_eq!(
            formula,
            "MAX(#1 - COALESCE(#2, #3, 0.0), 0.0) + COALESCE(MAX(#2 - #3, 0.0), 0.0)"
        );

        // Add a solar meter with two solar inverters to the grid meter.
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        assert_eq!(meter_pv_chain.component_id(), 5);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of grid meter from all its suceessors
                "MAX(",
                "#1 - COALESCE(#2, #3, 0.0) - COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)), ",
                "0.0",
                ") + ",
                // difference of battery meter from battery inverter and pv
                // meter from the two pv inverters.
                "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0)",
            )
        );

        // Add a "mixed" meter with a CHP, an ev charger and a solar inverter to
        // the grid meter.
        let solar_inverter = builder.solar_inverter();
        let chp = builder.chp();
        let ev_charger = builder.ev_charger();
        let meter = builder.meter();
        builder.connect(meter, solar_inverter);
        builder.connect(meter, chp);
        builder.connect(meter, ev_charger);
        builder.connect(grid_meter, meter);

        assert_eq!(solar_inverter.component_id(), 8);
        assert_eq!(chp.component_id(), 9);
        assert_eq!(ev_charger.component_id(), 10);
        assert_eq!(meter.component_id(), 11);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of grid meter from all its suceessors
                "MAX(",
                "#1 - ",
                "COALESCE(#2, #3, 0.0) - ",
                "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
                "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)), ",
                "0.0) + ",
                // difference of battery meter from battery inverter and pv
                // meter from the two pv inverters.
                "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
                // difference of "mixed" meter from its successors.
                "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0)"
            )
        );
        let graph = builder.build(Some(ComponentGraphConfig {
            disable_fallback_components: true,
            ..Default::default()
        }))?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of grid meter from all its suceessors (without fallbacks)
                "MAX(#1 - #2 - #5 - #11, 0.0) + ",
                // difference of battery meter from battery inverter and pv
                // meter from the two pv inverters.
                "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
                // difference of "mixed" meter from its successors.
                "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0)"
            )
        );

        // add a battery chain to the grid meter and a dangling meter to the grid.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        let dangling_meter = builder.meter();
        builder.connect(grid_meter, meter_bat_chain);
        builder.connect(grid, dangling_meter);

        assert_eq!(meter_bat_chain.component_id(), 12);
        assert_eq!(dangling_meter.component_id(), 15);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of grid meter from all its suceessors
                "MAX(",
                "#1 - ",
                "COALESCE(#2, #3, 0.0) - ",
                "COALESCE(#5, COALESCE(#7, 0.0) + COALESCE(#6, 0.0)) - ",
                "COALESCE(#11, COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)) - ",
                "COALESCE(#12, #13, 0.0), ",
                "0.0) + ",
                // difference of battery meter from battery inverter and pv
                // meter from the two pv inverters.
                "COALESCE(MAX(#2 - #3, 0.0), 0.0) + COALESCE(MAX(#5 - #6 - #7, 0.0), 0.0) + ",
                // difference of "mixed" meter from its successors.
                "COALESCE(MAX(#11 - #8 - #9 - #10, 0.0), 0.0) + ",
                // difference of second battery meter from inverter.
                "COALESCE(MAX(#12 - #13, 0.0), 0.0) + ",
                // consumption component of the dangling meter.
                "MAX(#15, 0.0)"
            )
        );

        Ok(())
    }

    #[test]
    fn test_consumer_formula_without_grid_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a meter-inverter-battery chain to the grid component.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 1);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        // Formula subtracts inverter from battery meter, or shows zero
        // consumption if either of the components have no data.
        assert_eq!(formula, "COALESCE(MAX(#1 - #2, 0.0), 0.0)");

        // Add a pv meter with one solar inverter and two dangling meter.
        let meter_pv_chain = builder.meter_pv_chain(1);
        let dangling_meter_1 = builder.meter();
        let dangling_meter_2 = builder.meter();
        builder.connect(grid, meter_pv_chain);
        builder.connect(grid, dangling_meter_1);
        builder.connect(grid, dangling_meter_2);

        assert_eq!(meter_pv_chain.component_id(), 4);
        assert_eq!(dangling_meter_1.component_id(), 6);
        assert_eq!(dangling_meter_2.component_id(), 7);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // subtract meter successors from meters
                "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
                // dangling meters
                "MAX(#6, 0.0) + MAX(#7, 0.0)"
            )
        );

        // Add a battery inverter to the grid, without a battery meter.
        //
        // This shouldn't show up in the formula, because battery inverter
        // consumption is charging, not site consumption.
        let inv_bat_chain = builder.inv_bat_chain(1);
        builder.connect(grid, inv_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // subtract meter successors from meters
                "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
                // dangling meters
                "MAX(#6, 0.0) + MAX(#7, 0.0)"
            )
        );

        // Add a PV inverter and a CHP to the grid, without a meter.
        //
        // Their consumption is counted as site consumption, because they can't
        // be taken out, by discharging the batteries, for example.
        let pv_inv = builder.solar_inverter();
        let chp = builder.chp();
        builder.connect(grid, pv_inv);
        builder.connect(grid, chp);

        assert_eq!(pv_inv.component_id(), 10);
        assert_eq!(chp.component_id(), 11);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // subtract meter successors from meters
                "COALESCE(MAX(#1 - #2, 0.0), 0.0) + COALESCE(MAX(#4 - #5, 0.0), 0.0) + ",
                // dangling meters
                "MAX(#6, 0.0) + MAX(#7, 0.0) + ",
                // PV inverter and CHP
                "MAX(#11, 0.0) + MAX(#10, 0.0)",
            )
        );

        Ok(())
    }

    #[test]
    fn test_consumer_formula_diamond_meters() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add three meters to the grid
        let grid_meter_1 = builder.meter();
        let grid_meter_2 = builder.meter();
        let grid_meter_3 = builder.meter();
        builder.connect(grid, grid_meter_1);
        builder.connect(grid, grid_meter_2);
        builder.connect(grid, grid_meter_3);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(formula, "MAX(#1, 0.0) + MAX(#2, 0.0) + MAX(#3, 0.0)");

        // Add two solar inverters with two grid meters as predecessors.
        let meter_pv_chain_1 = builder.meter_pv_chain(1);
        let meter_pv_chain_2 = builder.meter_pv_chain(1);
        builder.connect(grid_meter_1, meter_pv_chain_1);
        builder.connect(grid_meter_1, meter_pv_chain_2);
        builder.connect(grid_meter_2, meter_pv_chain_1);
        builder.connect(grid_meter_2, meter_pv_chain_2);

        assert_eq!(meter_pv_chain_1.component_id(), 4);
        assert_eq!(meter_pv_chain_2.component_id(), 6);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of pv powers from first two grid meters
                "MAX(#1 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0), 0.0) + ",
                // third grid meter still dangling
                "MAX(#3, 0.0) + ",
                // difference of solar inverters from their meters
                "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0)"
            )
        );

        // Add a meter to grid meter 3, and then add the two solar inverters to
        // that meter.
        let meter = builder.meter();
        builder.connect(grid_meter_3, meter);
        builder.connect(meter, meter_pv_chain_1);
        builder.connect(meter, meter_pv_chain_2);

        assert_eq!(meter.component_id(), 8);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of pv powers from first two grid meters and meter#8
                "MAX(#1 + #8 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0), 0.0) + ",
                // difference of meter#8 from third grid meter
                "MAX(#3 - #8, 0.0) + ",
                // difference of solar inverters from their meters
                "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0)"
            )
        );

        // Add a battery inverter to the first grid meter.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter_1, meter_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.consumer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                // difference of pv and battery powers from first two grid
                // meters and meter#8
                "MAX(",
                "#1 + #8 + #2 - COALESCE(#4, #5, 0.0) - COALESCE(#6, #7, 0.0) - COALESCE(#9, #10, 0.0), ",
                "0.0) + ",
                // difference of meter#8 from third grid meter
                "MAX(#3 - #8, 0.0) + ",
                // difference of solar inverters from their meters
                "COALESCE(MAX(#4 - #5, 0.0), 0.0) + COALESCE(MAX(#6 - #7, 0.0), 0.0) + ",
                // difference of battery inverter from battery meter
                "COALESCE(MAX(#9 - #10, 0.0), 0.0)"
            )
        );

        Ok(())
    }
}
