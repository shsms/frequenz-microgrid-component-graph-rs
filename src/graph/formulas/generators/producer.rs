// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use super::super::expr::Expr;
use crate::component_category::CategoryPredicates;
use crate::graph::formulas::Formula;
use crate::graph::formulas::fallback::{SourcePreference, aggregate_terms};
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
    /// the graph. Wind turbines and steam boilers have per-category formulas of
    /// their own and are not part of this sum.
    pub fn build(self) -> Result<Formula, Error> {
        // One aggregation over the component set, so a component fed
        // through two meters — or shared between two PV meters — is
        // measured once. Per-meter aggregation counted such feeds twice
        // and dropped shared components.
        let targets = self.graph.find_all(
            self.graph.root_id,
            |node| node.is_pv_inverter() || node.is_chp(),
            petgraph::Direction::Outgoing,
            false,
        )?;
        let expr = aggregate_terms(self.graph, targets, SourcePreference::ComponentsFirst)?
            .into_iter()
            .reduce(|sum, term| sum + term)
            .unwrap_or_else(|| Expr::number(0.0));
        Ok(Formula::new(expr))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Error, graph::test_utils::graph};

    #[test]
    fn test_producer_formula() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: meter } }.build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(formula, "0.0");

        // A PV meter with two PV inverters behind the grid meter.
        let graph = graph! {
            0: grid => { 1: meter => { 2: meter => { 3: inv_pv, 4: inv_pv } } },
        }
        .build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0))"
        );

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_pv, 4: inv_pv } },
                // A CHP meter on the grid with a CHP behind it.
                5: meter => { 6: chp },
            },
        }
        .build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)) + ",
                "COALESCE(#6, #5, 0.0)"
            )
        );

        let graph = graph! {
            0: grid => {
                1: meter => { 2: meter => { 3: inv_pv, 4: inv_pv } },
                5: meter => { 6: chp },
                // A CHP on the grid, without a meter.
                7: chp,
            },
        }
        .build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#7, 0.0)"
            )
        );

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_pv, 4: inv_pv },
                    // A PV inverter on the grid meter.
                    8: inv_pv,
                },
                5: meter => { 6: chp },
                7: chp,
            },
        }
        .build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#7, 0.0) + ",
                "COALESCE(#8, 0.0)"
            )
        );

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_pv, 4: inv_pv },
                    8: inv_pv,
                    // A battery chain on the grid meter.
                    9: meter => { 10: inv_bat => { 11: bat } },
                },
                5: meter => { 6: chp },
                7: chp,
            },
        }
        .build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#7, 0.0) + ",
                "COALESCE(#8, 0.0)"
            )
        );

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_pv, 4: inv_pv },
                    8: inv_pv,
                    9: meter => { 10: inv_bat => { 11: bat } },
                    // A meter on the grid meter, with a PV inverter and a CHP
                    // behind it. The pair is measured by one term, as
                    // same-meter siblings of one category are, so the meter's
                    // reading stands in for the pair even when neither
                    // component sends data.
                    12: meter => { 13: inv_pv, 14: chp },
                },
                5: meter => { 6: chp },
                7: chp,
            },
        }
        .build(None)?;
        let formula = graph.producer_formula()?.to_string();
        assert_eq!(
            formula,
            concat!(
                "COALESCE(#4 + #3, #2, COALESCE(#4, 0.0) + COALESCE(#3, 0.0)) + ",
                "COALESCE(#6, #5, 0.0) + ",
                "COALESCE(#7, 0.0) + ",
                "COALESCE(#8, 0.0) + ",
                "COALESCE(#14 + #13, #12, COALESCE(#14, 0.0) + COALESCE(#13, 0.0))"
            )
        );

        Ok(())
    }

    /// A PV inverter shared by two PV meters is measured once, by its own
    /// reading with the meter diamond as fallback. Per-meter aggregation
    /// dropped the shared inverter from both meters' terms.
    #[test]
    fn test_producer_formula_measures_a_shared_inverter_once() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter => { 3: inv_pv, 4: inv_pv },
                2: meter => { 4, 5: inv_pv },
            },
        }
        .build(None)?;
        assert_eq!(
            graph.producer_formula()?.to_string(),
            "COALESCE(#3 + #4 + #5, COALESCE(#1, 0.0) + COALESCE(#2, 0.0))"
        );

        Ok(())
    }

    /// A PV inverter fed from two meters is not counted through both: its
    /// own reading covers both feeds. Per-meter aggregation added the
    /// pure-PV meter's reading on top of the inverter's.
    #[test]
    fn test_producer_formula_counts_a_double_fed_inverter_once() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter => { 3: inv_pv },
                2: meter => { 3, 4: ev },
            },
        }
        .build(None)?;
        assert_eq!(graph.producer_formula()?.to_string(), "COALESCE(#3, 0.0)");

        Ok(())
    }
}
