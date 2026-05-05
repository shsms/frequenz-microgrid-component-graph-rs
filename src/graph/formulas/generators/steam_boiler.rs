// License: MIT
// Copyright © 2026 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating steam boiler formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::AggregationFormula;
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::FallbackExpr;
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct SteamBoilerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    steam_boiler_ids: BTreeSet<u64>,
}

impl<'a, N, E> SteamBoilerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        steam_boiler_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let steam_boiler_ids = if let Some(steam_boiler_ids) = steam_boiler_ids {
            steam_boiler_ids
        } else {
            graph.find_all(
                graph.root_id,
                |node| node.is_steam_boiler(),
                petgraph::Direction::Outgoing,
                false,
            )?
        };
        Ok(Self {
            graph,
            steam_boiler_ids,
        })
    }

    /// Generates the steam boiler formula.
    ///
    /// This is the sum of all steam boilers in the graph. If the steam_boiler_ids are provided,
    /// only the steam boilers with the given ids are included in the formula.
    pub fn build(self) -> Result<AggregationFormula, Error> {
        if self.steam_boiler_ids.is_empty() {
            return Ok(AggregationFormula::new(Expr::number(0.0)));
        }

        for id in &self.steam_boiler_ids {
            if !self.graph.component(*id)?.is_steam_boiler() {
                return Err(Error::invalid_component(format!(
                    "Component with id {id} is not a steam boiler."
                )));
            }
        }

        FallbackExpr::new()
            .prefer_meters(self.graph.config.prefer_meters_in_steam_boiler_formula())
            .generate(self.graph, self.steam_boiler_ids.clone())
            .map(AggregationFormula::new)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{Error, graph::test_utils::ComponentGraphBuilder};

    #[test]
    fn test_steam_boiler_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let prefer_steam_boiler_config = Some(crate::ComponentGraphConfig {
            formula_overrides: crate::FormulaOverrides::builder()
                .prefer_meters_in_steam_boiler_formula(false)
                .build(),
            ..Default::default()
        });

        let graph = builder.build(prefer_steam_boiler_config.clone())?;
        let formula = graph.steam_boiler_formula(None)?.to_string();
        assert_eq!(formula, "0.0");

        // Add a steam boiler meter with one steam boiler.
        let meter_steam_boiler_chain = builder.meter_steam_boiler_chain(1);
        builder.connect(grid_meter, meter_steam_boiler_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_steam_boiler_chain.component_id(), 2);

        let graph = builder.build(prefer_steam_boiler_config.clone())?;
        let formula = graph.steam_boiler_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        // Add a steam boiler meter with two steam boilers.
        let meter_steam_boiler_chain = builder.meter_steam_boiler_chain(2);
        builder.connect(grid_meter, meter_steam_boiler_chain);

        assert_eq!(meter_steam_boiler_chain.component_id(), 4);

        let graph = builder.build(prefer_steam_boiler_config.clone())?;
        let formula = graph.steam_boiler_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6 + #5, #4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0))"
            )
        );

        let formula = graph
            .steam_boiler_formula(Some(BTreeSet::from([6, 3])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0) + COALESCE(#6, 0.0)");

        // add a meter direct to the grid with three steam boilers
        let meter_steam_boiler_chain = builder.meter_steam_boiler_chain(3);
        builder.connect(grid, meter_steam_boiler_chain);

        assert_eq!(meter_steam_boiler_chain.component_id(), 7);

        let graph = builder.build(prefer_steam_boiler_config)?;
        let graph_prefer_meters = builder.build(None)?;

        let formula = graph.steam_boiler_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6 + #5, #4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0)) + ",
                "COALESCE(",
                "#10 + #9 + #8, ",
                "#7, ",
                "COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)",
                ")"
            ),
        );
        let formula = graph_prefer_meters.steam_boiler_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0)) + ",
                "COALESCE(",
                "#7, ",
                "COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)",
                ")"
            ),
        );

        let formula = graph
            .steam_boiler_formula(Some(BTreeSet::from([3, 5, 6, 8, 9])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6 + #5, #4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0)) + ",
                "COALESCE(#8, 0.0) + ",
                "COALESCE(#9, 0.0)"
            )
        );
        let formula = graph_prefer_meters
            .steam_boiler_formula(Some(BTreeSet::from([3, 5, 6, 8, 9])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0)) + ",
                "COALESCE(#8, 0.0) + ",
                "COALESCE(#9, 0.0)"
            )
        );

        let formula = graph
            .steam_boiler_formula(Some(BTreeSet::from([3, 5, 6, 8, 9, 10])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#6 + #5, #4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0)) + ",
                "COALESCE(",
                "#10 + #9 + #8, ",
                "#7, ",
                "COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)",
                ")"
            )
        );
        let formula = graph_prefer_meters
            .steam_boiler_formula(Some(BTreeSet::from([3, 5, 6, 8, 9, 10])))?
            .to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#2, #3, 0.0) + ",
                "COALESCE(#4, COALESCE(#6, 0.0) + COALESCE(#5, 0.0)) + ",
                "COALESCE(",
                "#7, ",
                "COALESCE(#10, 0.0) + COALESCE(#9, 0.0) + COALESCE(#8, 0.0)",
                ")"
            )
        );

        let formula = graph
            .steam_boiler_formula(Some(BTreeSet::from([6, 10])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#6, 0.0) + COALESCE(#10, 0.0)");
        let formula = graph_prefer_meters
            .steam_boiler_formula(Some(BTreeSet::from([6, 10])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#6, 0.0) + COALESCE(#10, 0.0)");

        // Failure cases:
        let formula = graph.steam_boiler_formula(Some(BTreeSet::from([4])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 4 is not a steam boiler."
        );
        let formula = graph_prefer_meters.steam_boiler_formula(Some(BTreeSet::from([4])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 4 is not a steam boiler."
        );

        Ok(())
    }
}
