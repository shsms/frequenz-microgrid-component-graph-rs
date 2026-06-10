// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::Formula;
use crate::graph::formulas::expr::Expr;
use crate::graph::formulas::fallback::FallbackExpr;
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct EVChargerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    ev_charger_ids: BTreeSet<u64>,
}

impl<'a, N, E> EVChargerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        ev_charger_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let ev_charger_ids = if let Some(ev_charger_ids) = ev_charger_ids {
            ev_charger_ids
        } else {
            graph.find_all(
                graph.root_id,
                |node| node.is_ev_charger(),
                petgraph::Direction::Outgoing,
                false,
            )?
        };
        Ok(Self {
            graph,
            ev_charger_ids,
        })
    }

    /// Generates the EV charger formula.
    ///
    /// This is the sum of all EV chargers in the graph. If the ev_charger_ids are provided,
    /// only the EV chargers with the given ids are included in the formula.
    pub fn build(self) -> Result<Formula, Error> {
        if self.ev_charger_ids.is_empty() {
            return Ok(Formula::new(Expr::number(0.0)));
        }

        for id in &self.ev_charger_ids {
            if !self.graph.component(*id)?.is_ev_charger() {
                return Err(Error::invalid_component(format!(
                    "Component with id {id} is not an EV charger."
                )));
            }
        }

        FallbackExpr::new()
            .prefer_meters(self.graph.config.prefer_meters_in_ev_charger_formula())
            .generate(self.graph, self.ev_charger_ids.clone())
            .map(Formula::new)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{Error, graph::test_utils::ComponentGraphBuilder};

    #[test]
    fn test_ev_charger_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let prefer_ev_charger_config = Some(
            crate::ComponentGraphConfig::builder()
                .formula_overrides(
                    crate::FormulaOverrides::builder()
                        .prefer_meters_in_ev_charger_formula(false)
                        .build(),
                )
                .build(),
        );

        let graph = builder.build(prefer_ev_charger_config.clone())?;
        let formula = graph.ev_charger_formula(None)?.to_string();
        assert_eq!(formula, "0.0");

        // Add a EV charger meter with one EV charger.
        let meter_ev_charger_chain = builder.meter_ev_charger_chain(1);
        builder.connect(grid_meter, meter_ev_charger_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_ev_charger_chain.component_id(), 2);

        let graph = builder.build(prefer_ev_charger_config.clone())?;
        let formula = graph.ev_charger_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        // Add a battery meter with one inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 4);

        let graph = builder.build(prefer_ev_charger_config.clone())?;
        let formula = graph.ev_charger_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

        // Add a EV charger meter with two EV chargers.
        let meter_ev_charger_chain = builder.meter_ev_charger_chain(2);
        builder.connect(grid_meter, meter_ev_charger_chain);

        assert_eq!(meter_ev_charger_chain.component_id(), 8);

        let graph = builder.build(prefer_ev_charger_config.clone())?;
        let formula = graph.ev_charger_formula(None)?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#3, #2, 0.0) + ",
                "COALESCE(#10 + #9, #8, COALESCE(#10, 0.0) + COALESCE(#9, 0.0))"
            )
        );

        let formula = graph
            .ev_charger_formula(Some(BTreeSet::from([10, 3])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0) + COALESCE(#10, 0.0)");

        // add a meter direct to the grid with three EV chargers
        let meter_ev_charger_chain = builder.meter_ev_charger_chain(3);
        builder.connect(grid, meter_ev_charger_chain);

        assert_eq!(meter_ev_charger_chain.component_id(), 11);

        let graph = builder.build(prefer_ev_charger_config)?;
        let graph_prefer_meters = builder.build(None)?;

        let formula = graph.ev_charger_formula(None)?.to_string();
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
        let formula = graph_prefer_meters.ev_charger_formula(None)?.to_string();
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
            .ev_charger_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))?
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
        let formula = graph_prefer_meters
            .ev_charger_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))?
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
            .ev_charger_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))?
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
        let formula = graph_prefer_meters
            .ev_charger_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))?
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
            .ev_charger_formula(Some(BTreeSet::from([10, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#10, 0.0) + COALESCE(#14, 0.0)");
        let formula = graph_prefer_meters
            .ev_charger_formula(Some(BTreeSet::from([10, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#10, 0.0) + COALESCE(#14, 0.0)");

        // Failure cases:
        let formula = graph.ev_charger_formula(Some(BTreeSet::from([8])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 8 is not an EV charger."
        );
        let formula = graph_prefer_meters.ev_charger_formula(Some(BTreeSet::from([8])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 8 is not an EV charger."
        );

        Ok(())
    }
}
