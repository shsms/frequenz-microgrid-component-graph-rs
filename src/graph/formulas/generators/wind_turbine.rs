// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::AggregationFormula;
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::FallbackExpr;
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct WindTurbineFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    wind_turbine_ids: BTreeSet<u64>,
}

impl<'a, N, E> WindTurbineFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        wind_turbine_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let wind_turbine_ids = if let Some(wind_turbine_ids) = wind_turbine_ids {
            wind_turbine_ids
        } else {
            graph.find_all(
                graph.root_id,
                |node| node.is_wind_turbine(),
                petgraph::Direction::Outgoing,
                false,
            )?
        };
        Ok(Self {
            graph,
            wind_turbine_ids,
        })
    }

    /// Generates the wind_turbine formula.
    ///
    /// This is the sum of all wind_turbines in the graph. If the wind_turbine_ids are provided,
    /// only the wind_turbines with the given ids are included in the formula.
    pub fn build(self) -> Result<AggregationFormula, Error> {
        if self.wind_turbine_ids.is_empty() {
            return Ok(AggregationFormula::new(Expr::number(0.0)));
        }

        for id in &self.wind_turbine_ids {
            if !self.graph.component(*id)?.is_wind_turbine() {
                return Err(Error::invalid_component(format!(
                    "Component with id {id} is not a wind turbine."
                )));
            }
        }

        FallbackExpr::new()
            .prefer_meters(self.graph.config.prefer_meters_in_wind_turbine_formula())
            .generate(self.graph, self.wind_turbine_ids.clone())
            .map(AggregationFormula::new)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        ComponentGraphConfig, Error, FormulaOverrides, graph::test_utils::ComponentGraphBuilder,
    };

    #[test]
    fn test_wind_turbine_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let prefer_wind_config = Some(ComponentGraphConfig {
            formula_overrides: FormulaOverrides::builder()
                .prefer_meters_in_wind_turbine_formula(false)
                .build(),
            ..Default::default()
        });

        let graph = builder.build(prefer_wind_config.clone())?;
        let formula = graph.wind_turbine_formula(None)?.to_string();
        assert_eq!(formula, "0.0");

        // Add a wind_turbine meter with one wind turbine.
        let meter_wind_turbine_chain = builder.meter_wind_turbine_chain(1);
        builder.connect(grid_meter, meter_wind_turbine_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_wind_turbine_chain.component_id(), 2);
        let graph = builder.build(prefer_wind_config.clone())?;
        let formula = graph.wind_turbine_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        // Add a battery meter with one battery inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 4);

        let graph = builder.build(prefer_wind_config.clone())?;
        let formula = graph.wind_turbine_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        // Add a wind_turbine meter with two wind_turbine inverters.
        let meter_wind_turbine_chain = builder.meter_wind_turbine_chain(2);
        builder.connect(grid_meter, meter_wind_turbine_chain);

        assert_eq!(meter_wind_turbine_chain.component_id(), 8);

        let graph = builder.build(prefer_wind_config.clone())?;
        let formula = graph.wind_turbine_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#10 + #9, #8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0))"
            )
        );

        let formula = graph
            .wind_turbine_formula(Some(BTreeSet::from([10, 3])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0) + COALESCE(#10, 0.0)");

        // add a meter direct to the grid with three wind_turbine inverters
        let meter_wind_turbine_chain = builder.meter_wind_turbine_chain(3);
        builder.connect(grid, meter_wind_turbine_chain);

        assert_eq!(meter_wind_turbine_chain.component_id(), 11);
        let graph = builder.build(prefer_wind_config)?;
        let graph_pref_meter = builder.build(None)?;
        let formula = graph.wind_turbine_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#10 + #9, #8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0)) + ",
                "COALESCE(",
                "#14 + #13 + #12, ",
                "#11, ",
                "COALESCE(#14, 0.0) + COALESCE(#13, 0.0) + COALESCE(#12, 0.0)",
                ")"
            ),
        );
        let formula = graph_pref_meter.wind_turbine_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0)) + ",
                "COALESCE(",
                "#11, ",
                "COALESCE(#14, 0.0) + COALESCE(#13, 0.0) + COALESCE(#12, 0.0)",
                ")"
            ),
        );

        let formula = graph
            .wind_turbine_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#10 + #9, #8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0)) + ",
                "COALESCE(#12, 0.0) + ",
                "COALESCE(#13, 0.0)"
            )
        );
        let formula = graph_pref_meter
            .wind_turbine_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0)) + ",
                "COALESCE(#12, 0.0) + ",
                "COALESCE(#13, 0.0)"
            )
        );

        let formula = graph
            .wind_turbine_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#10 + #9, #8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0)) + ",
                "COALESCE(",
                "#14 + #13 + #12, ",
                "#11, ",
                "COALESCE(#14, 0.0) + COALESCE(#13, 0.0) + COALESCE(#12, 0.0)",
                ")"
            )
        );
        let formula = graph_pref_meter
            .wind_turbine_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0)) + ",
                "COALESCE(",
                "#11, ",
                "COALESCE(#14, 0.0) + COALESCE(#13, 0.0) + COALESCE(#12, 0.0)",
                ")"
            )
        );

        let formula = graph
            .wind_turbine_formula(Some(BTreeSet::from([10, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#10, 0.0) + COALESCE(#14, 0.0)");
        let formula = graph_pref_meter
            .wind_turbine_formula(Some(BTreeSet::from([10, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#10, 0.0) + COALESCE(#14, 0.0)");

        // Failure cases:
        let formula = graph.wind_turbine_formula(Some(BTreeSet::from([8])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 8 is not a wind turbine."
        );

        Ok(())
    }
}
