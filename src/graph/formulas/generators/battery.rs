// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct BatteryFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    inverter_ids: BTreeSet<u64>,
}

impl<'a, N, E> BatteryFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let inverter_ids = if let Some(battery_ids) = battery_ids {
            Self::find_inverter_ids(graph, &battery_ids)?
        } else {
            graph.find_all(graph.root_id, |node| node.is_battery_inverter(), false)?
        };
        Ok(Self {
            graph,
            inverter_ids,
        })
    }

    /// Generates the battery formula.
    ///
    /// This is the sum of all battery_inverters in the graph. If the
    /// battery_ids are provided, only the batteries with the given ids are
    /// included in the formula.
    pub fn build(self) -> Result<String, Error> {
        if self.inverter_ids.is_empty() {
            return Ok("0.0".to_string());
        }

        self.graph
            .fallback_expr(self.inverter_ids, false)
            .map(|expr| expr.to_string())
    }

    fn find_inverter_ids(
        graph: &ComponentGraph<N, E>,
        battery_ids: &BTreeSet<u64>,
    ) -> Result<BTreeSet<u64>, Error> {
        let mut inverter_ids = BTreeSet::new();
        for battery_id in battery_ids {
            if !graph.component(*battery_id)?.is_battery() {
                return Err(Error::invalid_component(format!(
                    "Component with id {} is not a battery.",
                    battery_id
                )));
            }
            for sibling in graph.siblings_from_predecessors(*battery_id)? {
                if !battery_ids.contains(&sibling.component_id()) {
                    return Err(Error::invalid_component(format!(
                        "Battery {} can't be in a formula without all its siblings: {:?}.",
                        battery_id,
                        graph
                            .siblings_from_predecessors(*battery_id)?
                            .map(|x| x.component_id())
                            .collect::<Vec<_>>()
                    )));
                }
            }
            inverter_ids.extend(graph.predecessors(*battery_id)?.map(|x| x.component_id()));
        }
        Ok(inverter_ids)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{graph::test_utils::ComponentGraphBuilder, Error};

    #[test]
    fn test_battery_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build()?;
        let formula = graph.battery_formula(None)?;
        assert_eq!(formula, "0.0");

        // Add a battery meter with one inverter and one battery.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build()?;
        let formula = graph.battery_formula(None)?;
        assert_eq!(formula, "COALESCE(#3, #2)");

        // Add a second battery meter with one inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build()?;
        let formula = graph.battery_formula(None)?;
        assert_eq!(formula, "COALESCE(#3, #2) + COALESCE(#6, #5)");

        let formula = graph.battery_formula(Some(BTreeSet::from([4])))?;
        assert_eq!(formula, "COALESCE(#3, #2)");

        let formula = graph.battery_formula(Some(BTreeSet::from([7, 8])))?;
        assert_eq!(formula, "COALESCE(#6, #5)");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([4, 8, 7])))
            .unwrap();
        assert_eq!(formula, "COALESCE(#3, #2) + COALESCE(#6, #5)");

        // Add a third battery meter with two inverters with two connected batteries.
        let meter_bat_chain = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 9);

        let graph = builder.build()?;
        let formula = graph.battery_formula(None)?;
        assert_eq!(
            formula,
            "COALESCE(#3, #2) + COALESCE(#6, #5) + COALESCE(#11 + #10, #9)"
        );

        let formula = graph
            .battery_formula(Some(BTreeSet::from([12, 13])))
            .unwrap();
        assert_eq!(formula, "COALESCE(#11 + #10, #9)");

        // add a PV meter with two PV inverters.
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        assert_eq!(meter_pv_chain.component_id(), 14);

        let graph = builder.build()?;
        let formula = graph.battery_formula(None)?;
        assert_eq!(
            formula,
            "COALESCE(#3, #2) + COALESCE(#6, #5) + COALESCE(#11 + #10, #9)"
        );

        // add a battery meter with two inverters that have their own batteries.
        let meter = builder.meter();
        builder.connect(grid, meter);
        let inv_bat_chain = builder.inv_bat_chain(1);
        builder.connect(meter, inv_bat_chain);

        assert_eq!(meter.component_id(), 17);
        assert_eq!(inv_bat_chain.component_id(), 18);

        let inv_bat_chain = builder.inv_bat_chain(1);
        builder.connect(meter, inv_bat_chain);

        assert_eq!(inv_bat_chain.component_id(), 20);

        let graph = builder.build()?;
        let formula = graph.battery_formula(None)?;
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2) + COALESCE(#6, #5) + ",
                "COALESCE(#11 + #10, #9) + COALESCE(#20 + #18, #17)"
            )
        );

        let formula = graph
            .battery_formula(Some(BTreeSet::from([19, 21])))
            .unwrap();
        assert_eq!(formula, "COALESCE(#20 + #18, #17)");

        let formula = graph.battery_formula(Some(BTreeSet::from([19]))).unwrap();
        assert_eq!(formula, "#18");

        let formula = graph.battery_formula(Some(BTreeSet::from([21]))).unwrap();
        assert_eq!(formula, "#20");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([4, 12, 13, 19])))
            .unwrap();
        assert_eq!(formula, "COALESCE(#3, #2) + COALESCE(#11 + #10, #9) + #18");

        // Failure cases:
        let formula = graph.battery_formula(Some(BTreeSet::from([17])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 17 is not a battery."
        );

        let formula = graph.battery_formula(Some(BTreeSet::from([12])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Battery 12 can't be in a formula without all its siblings: [13]."
        );

        Ok(())
    }
}
