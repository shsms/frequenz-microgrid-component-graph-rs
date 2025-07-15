// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating battery AC coalesce formulas.

use crate::component_category::CategoryPredicates;
use std::collections::BTreeSet;

use crate::{
    graph::formulas::{expr::Expr, AggregationFormula},
    ComponentGraph, Edge, Error, Node,
};

use super::battery::BatteryFormulaBuilder;

pub(crate) struct BatteryAcCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    inverter_ids: BTreeSet<u64>,
}

impl<'a, N, E> BatteryAcCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        battery_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let inverter_ids = if let Some(ref battery_ids) = battery_ids {
            BatteryFormulaBuilder::find_inverter_ids(graph, battery_ids)?
        } else {
            graph.find_all(
                graph.root_id,
                |node| node.is_battery_inverter(&graph.config),
                petgraph::Direction::Outgoing,
                false,
            )?
        };
        Ok(Self {
            graph,
            inverter_ids,
        })
    }

    /// Generates the battery AC coalesce formula for the given battery IDs.
    ///
    /// This can be used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all the specified
    /// battery meters and inverters.
    ///
    /// When the `battery_ids` parameter is `None`, it will include all
    /// battery meters and inverters in the graph.
    pub fn build(self) -> Result<AggregationFormula, Error> {
        let mut meters: BTreeSet<u64> = BTreeSet::new();
        let mut source_components: Vec<Expr> = vec![];

        for inv_id in &self.inverter_ids {
            for pred in self.graph.predecessors(*inv_id)? {
                if self.graph.is_battery_meter(pred.component_id())? {
                    meters.insert(pred.component_id());
                }
            }
        }
        source_components.extend(
            meters
                .iter()
                .chain(self.inverter_ids.iter())
                .map(|component_id: &u64| Expr::component(*component_id)),
        );

        if source_components.is_empty() {
            return Err(Error::component_not_found("No battery inverters found."));
        }

        Ok(AggregationFormula::new(Expr::coalesce(source_components)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        graph::test_utils::ComponentGraphBuilder, ComponentGraphConfig, Error, InverterType,
    };

    #[test]
    fn test_battery_ac_voltage_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None);
        assert_eq!(
            formula,
            Err(Error::component_not_found("No battery inverters found."))
        );

        // Add a battery meter with one inverter and one battery.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");
        let formula = graph.battery_ac_coalesce_formula(Some(BTreeSet::from([12])));
        assert_eq!(
            formula,
            Err(Error::component_not_found(
                "Component with id 12 not found."
            ))
        );

        // Add a second battery meter with one inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #5, #3, #6)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([4])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([7, 8])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#5, #6)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([4, 8, 7])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #5, #3, #6)");

        // Add a third battery meter with two inverters with two connected batteries.
        let meter_bat_chain = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 9);

        let graph = builder.build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #5, #9, #3, #6, #10, #11)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([12, 13])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#9, #10, #11)");

        // add a PV meter with two PV inverters.
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        assert_eq!(meter_pv_chain.component_id(), 14);

        let graph = builder.build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #5, #9, #3, #6, #10, #11)");

        // add a battery meter with two inverters that have their own batteries.
        let meter = builder.meter();
        builder.connect(grid, meter);
        let inv_bat_chain = builder.inv_bat_chain(1);
        builder.connect(meter, inv_bat_chain);

        assert_eq!(meter.component_id(), 17);
        assert_eq!(inv_bat_chain.component_id(), 18);

        let unspec_inverter = builder.add_component(crate::ComponentCategory::Inverter(
            InverterType::Unspecified,
        ));
        let battery = builder.battery();
        builder.connect(unspec_inverter, battery);
        builder.connect(meter, unspec_inverter);

        assert_eq!(unspec_inverter.component_id(), 20);

        assert!(builder
            .build(None)
            .is_err_and(|x| x.to_string()
                == "InvalidComponent: InverterType not specified for inverter: 20"));

        let graph = builder.build(Some(ComponentGraphConfig {
            allow_unspecified_inverters: true,
            ..Default::default()
        }))?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(
            formula,
            "COALESCE(#2, #5, #9, #17, #3, #6, #10, #11, #18, #20)"
        );

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([19, 21])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#17, #18, #20)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([19])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#17, #18)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([21])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#17, #20)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([4, 12, 13, 19])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #9, #17, #3, #10, #11, #18)");

        // Failure cases:
        let formula = graph.battery_ac_coalesce_formula(Some(BTreeSet::from([17])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 17 is not a battery."
        );

        let formula = graph.battery_ac_coalesce_formula(Some(BTreeSet::from([12])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Battery 12 can't be in a formula without all its siblings: [13]."
        );

        Ok(())
    }
}
