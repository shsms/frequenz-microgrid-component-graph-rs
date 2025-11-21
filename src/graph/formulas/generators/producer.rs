// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use super::super::expr::Expr;
use crate::component_category::CategoryPredicates;
use crate::graph::formulas::fallback::FallbackExpr;
use crate::graph::formulas::AggregationFormula;
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct ProducerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
}

impl<'a, N, E> ProducerFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(graph: &'a ComponentGraph<N, E>) -> Result<Self, Error> {
        Ok(Self { graph })
    }

    /// Generates the production formula.
    ///
    /// The production formula is the sum of all the PV and CHP components in
    /// the graph.
    pub fn build(self) -> Result<AggregationFormula, Error> {
        let mut expr = None;
        for component_id in self.graph.find_all(
            self.graph.root_id,
            |node| {
                self.graph.is_pv_meter(node.component_id()).unwrap_or(false)
                    || self
                        .graph
                        .is_chp_meter(node.component_id())
                        .unwrap_or(false)
                    || node.is_pv_inverter()
                    || node.is_chp()
            },
            petgraph::Direction::Outgoing,
            false,
        )? {
            let comp_expr = FallbackExpr {
                prefer_meters: false,
                meter_fallback_for_meters: false,
            }
            .generate(self.graph, BTreeSet::from([component_id]))?
            .min(Expr::number(0.0));
            expr = match expr {
                None => Some(comp_expr),
                Some(e) => Some(e + comp_expr),
            };
        }
        Ok(expr
            .map(AggregationFormula::new)
            .unwrap_or_else(|| AggregationFormula::new(Expr::number(0.0))))
    }
}

#[cfg(test)]
mod tests {
    use crate::{graph::test_utils::ComponentGraphBuilder, Error};

    #[test]
    fn test_producer_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a grid meter and a PV meter with two PV inverters behind it.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(formula, "0.0");

        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0)"
        );

        // Add a CHP meter to the grid with a CHP behind it.
        let meter_chp_chain = builder.meter_chp_chain(1);
        builder.connect(grid, meter_chp_chain);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0)"
            )
        );

        // Add a CHP to the grid, without a meter.
        let chp = builder.chp();
        builder.connect(grid, chp);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0)"
            )
        );

        // Add a PV inverter to the grid_meter.
        let pv_inverter = builder.solar_inverter();
        builder.connect(grid_meter, pv_inverter);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0) + ",
                "MIN(COALESCE(#8, 0.0), 0.0)"
            )
        );

        // Add a battery chain to the grid meter.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0) + ",
                "MIN(COALESCE(#8, 0.0), 0.0)"
            )
        );

        // Add a meter to the grid meter, that has a PV inverter and a CHP behind it.
        let meter = builder.meter();
        let pv_inverter = builder.solar_inverter();
        let chp = builder.chp();
        builder.connect(meter, pv_inverter);
        builder.connect(meter, chp);
        builder.connect(grid_meter, meter);

        let graph = builder.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "MIN(COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)), 0.0) + ",
                "MIN(COALESCE(#6, #5, 0.0), 0.0) + ",
                "MIN(COALESCE(#7, 0.0), 0.0) + ",
                "MIN(COALESCE(#8, 0.0), 0.0) + ",
                "MIN(COALESCE(#13, 0.0), 0.0) + ",
                "MIN(COALESCE(#14, 0.0), 0.0)"
            )
        );

        Ok(())
    }
}
