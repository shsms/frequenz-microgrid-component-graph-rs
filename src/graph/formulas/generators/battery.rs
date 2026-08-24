// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::Formula;
use crate::graph::formulas::explain::{Explained, ExplanationKind};
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::{SourcePreference, aggregate};
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

    /// Generates the battery formula.
    ///
    /// This is the sum of all battery_inverters in the graph. If the
    /// battery_ids are provided, only the batteries with the given ids are
    /// included in the formula.
    pub fn build(self) -> Result<Formula, Error> {
        Ok(Formula::new(self.build_explained()?.expr))
    }

    /// Like [`Self::build`], but also explains each formula part.
    pub fn build_explained(self) -> Result<Explained, Error> {
        if self.inverter_ids.is_empty() {
            return Ok(Explained::leaf(
                Expr::number(0.0),
                ExplanationKind::DefaultZero,
                "No battery inverters are included, so the battery power is 0.0.",
            ));
        }

        aggregate(
            self.graph,
            self.inverter_ids.clone(),
            SourcePreference::prefer_meters(self.graph.config.prefer_meters_in_battery_formula()),
        )
    }

    pub(super) fn find_inverter_ids(
        graph: &ComponentGraph<N, E>,
        battery_ids: &BTreeSet<u64>,
    ) -> Result<BTreeSet<u64>, Error> {
        let mut inverter_ids = BTreeSet::new();
        for battery_id in battery_ids {
            if !graph.component(*battery_id)?.is_battery() {
                return Err(Error::invalid_component(format!(
                    "Component with id {battery_id} is not a battery."
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
            // Only battery inverters are battery-power sources. A hybrid
            // inverter's AC reading mixes in its PV production, so it is
            // left out — the same stance the no-selection path takes.
            inverter_ids.extend(
                graph
                    .predecessors(*battery_id)?
                    .filter(|x| x.is_battery_inverter(&graph.config))
                    .map(|x| x.component_id()),
            );
        }
        Ok(inverter_ids)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        ComponentGraphConfig, Error, FormulaOverrides, InverterType,
        graph::test_utils::ComponentGraphBuilder,
    };

    #[test]
    fn test_battery_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        // The global setting is meter-first. The battery override flips it
        // back to component-first. The component-first assertions below hold
        // only because the per-formula override wins over the global setting.
        let prefer_inverters_config = Some(
            ComponentGraphConfig::builder()
                .prefer_meters_in_component_formulas(true)
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_battery_formula(false)
                        .build(),
                )
                .build(),
        );

        let graph = builder.build(prefer_inverters_config.clone())?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(formula, "0.0");

        // Add a battery meter with one inverter and one battery.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_bat_chain.component_id(), 2);

        let graph = builder.build(prefer_inverters_config.clone())?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        // Add a second battery meter with one inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 5);

        let graph = builder.build(prefer_inverters_config.clone())?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0) + COALESCE(#6, #5, 0.0)");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([4])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([7, 8])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#6, #5, 0.0)");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([4, 8, 7])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0) + COALESCE(#6, #5, 0.0)");

        // Add a third battery meter with two inverters with two connected batteries.
        let meter_bat_chain = builder.meter_bat_chain(2, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 9);

        let graph = builder.build(prefer_inverters_config.clone())?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#11 + #10, #9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0))"
            )
        );

        let formula = graph
            .battery_formula(Some(BTreeSet::from([12, 13])))?
            .to_string();
        assert_eq!(
            formula,
            "COALESCE(#11 + #10, #9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0))"
        );

        // add a PV meter with two PV inverters.
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        assert_eq!(meter_pv_chain.component_id(), 14);

        let graph = builder.build(prefer_inverters_config)?;
        let graph_prefer_meters = builder.build(Some(
            ComponentGraphConfig::builder()
                .prefer_meters_in_component_formulas(true)
                .build(),
        ))?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#11 + #10, #9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0))"
            )
        );
        let formula = graph_prefer_meters.battery_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, #6, 0.0) + ",
                "COALESCE(#9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0))"
            )
        );

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

        assert!(
            builder.build(None).is_err_and(|x| x.to_string()
                == "InvalidComponent: InverterType not specified for inverter: 20")
        );

        // As above: the component-first assertions hold only because the
        // per-battery override wins over the global meter-first setting.
        let graph = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_unspecified_inverters(true)
                .prefer_meters_in_component_formulas(true)
                .formula_overrides(
                    FormulaOverrides::builder()
                        .prefer_meters_in_battery_formula(false)
                        .build(),
                )
                .build(),
        ))?;
        let graph_prefer_meters = builder.build(Some(
            ComponentGraphConfig::builder()
                .allow_unspecified_inverters(true)
                .prefer_meters_in_component_formulas(true)
                .build(),
        ))?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#11 + #10, #9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0)) + ",
                "COALESCE(#20 + #18, #17, COALESCE(#20, 0.0) + COALESCE(#18, 0.0))"
            )
        );
        let formula = graph_prefer_meters.battery_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#5, #6, 0.0) + ",
                "COALESCE(#9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0)) + ",
                "COALESCE(#17, COALESCE(#20, 0.0) + COALESCE(#18, 0.0))"
            )
        );

        let formula = graph
            .battery_formula(Some(BTreeSet::from([19, 21])))?
            .to_string();
        assert_eq!(
            formula,
            "COALESCE(#20 + #18, #17, COALESCE(#20, 0.0) + COALESCE(#18, 0.0))"
        );
        let formula = graph_prefer_meters
            .battery_formula(Some(BTreeSet::from([19, 21])))?
            .to_string();
        assert_eq!(
            formula,
            "COALESCE(#17, COALESCE(#20, 0.0) + COALESCE(#18, 0.0))"
        );

        let formula = graph
            .battery_formula(Some(BTreeSet::from([19])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#18, #17 - #20, 0.0)");
        let formula = graph_prefer_meters
            .battery_formula(Some(BTreeSet::from([19])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#17 - #20, #18, 0.0)");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([21])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#20, #17 - #18, 0.0)");
        let formula = graph_prefer_meters
            .battery_formula(Some(BTreeSet::from([21])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#17 - #18, #20, 0.0)");

        let formula = graph
            .battery_formula(Some(BTreeSet::from([4, 12, 13, 19])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#11 + #10, #9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0)) + ",
                "COALESCE(#18, #17 - #20, 0.0)"
            )
        );
        let formula = graph_prefer_meters
            .battery_formula(Some(BTreeSet::from([4, 12, 13, 19])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#9, COALESCE(#11, 0.0) + COALESCE(#10, 0.0)) + ",
                "COALESCE(#17 - #20, #18, 0.0)"
            )
        );

        // Failure cases:
        let formula = graph.battery_formula(Some(BTreeSet::from([17])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 17 is not a battery."
        );
        let formula = graph_prefer_meters.battery_formula(Some(BTreeSet::from([17])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 17 is not a battery."
        );

        let formula = graph.battery_formula(Some(BTreeSet::from([12])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Battery 12 can't be in a formula without all its siblings: [13]."
        );
        let formula = graph_prefer_meters.battery_formula(Some(BTreeSet::from([12])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Battery 12 can't be in a formula without all its siblings: [13]."
        );

        Ok(())
    }

    /// A component is measured through its predecessor meter, found by walking
    /// past a pass-through node.
    ///
    /// Topology (component ids): `Grid:0 → Meter:1 → PT:2 → Inverter:3 → Battery:4`
    #[test]
    fn test_battery_formula_finds_meter_through_passthrough() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let meter = builder.meter();
        let pt = builder.power_transformer();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();

        builder.connect(grid, meter);
        builder.connect(meter, pt);
        builder.connect(pt, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        let formula = graph.battery_formula(None)?.to_string();
        // Inverter falls back to its effective predecessor meter,
        // walking past the transformer.
        assert_eq!(formula, "COALESCE(#3, #1, 0.0)");
        Ok(())
    }

    /// A hybrid inverter is not a battery-power source: its AC reading
    /// mixes in its PV production. Selecting the battery explicitly must
    /// agree with the no-selection path, which already leaves hybrids out.
    ///
    /// Topology (ids): `Grid:0 → Meter:1 → HybridInverter:2 → Battery:3`,
    /// then a second chain `Meter:1 → Meter:4 → BatteryInverter:5 →
    /// Battery:6`.
    #[test]
    fn test_battery_formula_ignores_a_hybrid_inverter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();
        let grid_meter = builder.meter();
        let hybrid =
            builder.add_component(crate::ComponentCategory::Inverter(InverterType::Hybrid));
        let hybrid_battery = builder.battery();
        builder.connect(grid, grid_meter);
        builder.connect(grid_meter, hybrid);
        builder.connect(hybrid, hybrid_battery);

        let graph = builder.build(None)?;
        assert_eq!(graph.battery_formula(None)?.to_string(), "0.0");
        assert_eq!(
            graph
                .battery_formula(Some(BTreeSet::from([3])))?
                .to_string(),
            "0.0"
        );

        // Next to a real battery chain, both entry points count only that
        // chain.
        let battery_meter = builder.meter();
        let inverter = builder.battery_inverter();
        let battery = builder.battery();
        builder.connect(grid_meter, battery_meter);
        builder.connect(battery_meter, inverter);
        builder.connect(inverter, battery);

        let graph = builder.build(None)?;
        assert_eq!(
            graph.battery_formula(None)?.to_string(),
            "COALESCE(#5, #4, 0.0)"
        );
        assert_eq!(
            graph
                .battery_formula(Some(BTreeSet::from([3, 6])))?
                .to_string(),
            "COALESCE(#5, #4, 0.0)"
        );

        Ok(())
    }
}
