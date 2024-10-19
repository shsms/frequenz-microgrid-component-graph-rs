// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct CHPFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    chp_ids: BTreeSet<u64>,
}

impl<'a, N, E> CHPFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        chp_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let chp_ids = if let Some(chp_ids) = chp_ids {
            chp_ids
        } else {
            graph.find_all(graph.root_id, |node| node.is_chp(), false)?
        };
        Ok(Self { graph, chp_ids })
    }

    /// Generates the chp formula.
    ///
    /// This is the sum of all CHPs in the graph. If the chp_ids are provided,
    /// only the CHPs with the given ids are included in the formula.
    pub fn build(self) -> Result<String, Error> {
        if self.chp_ids.is_empty() {
            return Ok("0.0".to_string());
        }

        for id in &self.chp_ids {
            if !self.graph.component(*id)?.is_chp() {
                return Err(Error::invalid_component(format!(
                    "Component with id {} is not a CHP.",
                    id
                )));
            }
        }

        self.graph
            .fallback_expr(self.chp_ids, false)
            .map(|expr| expr.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{graph::test_utils::ComponentGraphBuilder, Error};

    #[test]
    fn test_chp_formula() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build()?;
        let formula = graph.pv_formula(None)?;
        assert_eq!(formula, "0.0");

        // Add a chp meter with one chp
        let meter_chp_chain = builder.meter_chp_chain(1);
        builder.connect(grid_meter, meter_chp_chain);

        assert_eq!(grid_meter.component_id(), 1);
        assert_eq!(meter_chp_chain.component_id(), 2);

        let graph = builder.build()?;
        let formula = graph.chp_formula(None)?;
        assert_eq!(formula, "COALESCE(#3, #2)");

        // Add a battery meter with one inverter and two batteries.
        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid_meter, meter_bat_chain);

        assert_eq!(meter_bat_chain.component_id(), 4);

        let graph = builder.build()?;
        let formula = graph.chp_formula(None)?;
        assert_eq!(formula, "COALESCE(#3, #2)");

        // Add a chp meter with two CHPs.
        let meter_chp_chain = builder.meter_chp_chain(2);
        builder.connect(grid_meter, meter_chp_chain);

        assert_eq!(meter_chp_chain.component_id(), 8);

        let graph = builder.build()?;
        let formula = graph.chp_formula(None)?;
        assert_eq!(formula, "COALESCE(#3, #2) + COALESCE(#10 + #9, #8)");

        let formula = graph.chp_formula(Some(BTreeSet::from([10, 3]))).unwrap();
        assert_eq!(formula, "COALESCE(#3, #2) + #10");

        // add a meter direct to the grid with three CHPs
        let meter_chp_chain = builder.meter_chp_chain(3);
        builder.connect(grid, meter_chp_chain);

        assert_eq!(meter_chp_chain.component_id(), 11);

        let graph = builder.build()?;
        let formula = graph.chp_formula(None)?;
        assert_eq!(
            formula,
            "COALESCE(#3, #2) + COALESCE(#10 + #9, #8) + COALESCE(#14 + #13 + #12, #11)",
        );

        let formula = graph
            .chp_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))
            .unwrap();
        assert_eq!(
            formula,
            "COALESCE(#3, #2) + COALESCE(#10 + #9, #8) + #12 + #13"
        );

        let formula = graph
            .chp_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))
            .unwrap();
        assert_eq!(
            formula,
            "COALESCE(#3, #2) + COALESCE(#10 + #9, #8) + COALESCE(#14 + #13 + #12, #11)"
        );

        let formula = graph.chp_formula(Some(BTreeSet::from([10, 14]))).unwrap();
        assert_eq!(formula, "#10 + #14");

        // Failure cases:
        let formula = graph.chp_formula(Some(BTreeSet::from([8])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 8 is not a CHP."
        );

        Ok(())
    }
}
