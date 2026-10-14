// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating PV AC coalesce formulas.

use crate::component_category::CategoryPredicates;
use std::collections::BTreeSet;

use crate::{ComponentGraph, Edge, Error, Node, graph::formulas::Formula};

pub(crate) struct PVAcCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    graph: &'a ComponentGraph<N, E>,
    pv_inverter_ids: BTreeSet<u64>,
}

impl<'a, N, E> PVAcCoalesceFormulaBuilder<'a, N, E>
where
    N: Node,
    E: Edge,
{
    pub fn try_new(
        graph: &'a ComponentGraph<N, E>,
        pv_inverter_ids: Option<BTreeSet<u64>>,
    ) -> Result<Self, Error> {
        let pv_inverter_ids = if let Some(pv_inverter_ids) = pv_inverter_ids {
            pv_inverter_ids
        } else {
            graph.find_all(
                graph.root_id,
                |node| node.is_pv_inverter(),
                petgraph::Direction::Outgoing,
                false,
            )?
        };
        Ok(Self {
            graph,
            pv_inverter_ids,
        })
    }

    /// Generates the PV AC coalesce formula for the given PV inverter IDs.
    ///
    /// This can be used for non-aggregating metrics like AC voltage or
    /// frequency.
    ///
    /// The formula is a `COALESCE` expression that includes all the specified
    /// PV meters and inverters.
    ///
    /// When the `pv_inverter_ids` parameter is `None`, it will include all PV
    /// meters and inverters in the graph.
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn build(self) -> Result<Formula, Error> {
        let mut meters: BTreeSet<u64> = BTreeSet::new();

        for inv_id in &self.pv_inverter_ids {
            if !self.graph.component(*inv_id)?.is_pv_inverter() {
                return Err(Error::invalid_component(format!(
                    "Component with id {inv_id} is not a PV inverter."
                )));
            }
            for pred in self.graph.predecessors(*inv_id)? {
                if self.graph.is_pv_meter(pred.component_id())? {
                    meters.insert(pred.component_id());
                }
            }
        }

        let coalesced = super::coalesce_with_telemetry(
            self.graph,
            meters.into_iter().chain(self.pv_inverter_ids),
        )?;

        Ok(Formula::new(coalesced))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{Error, graph::test_utils::graph};

    #[test]
    fn test_pv_ac_coalesce_formula() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: meter } }.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "None");

        // A PV meter with one PV inverter.
        let graph =
            graph! { 0: grid => { 1: meter => { 2: meter => { 3: inv_pv } } } }.build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_pv },
                    // A battery meter with one inverter and two batteries.
                    4: meter => { 5: inv_bat => { 6: bat, 7: bat } },
                },
            },
        }
        .build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_pv },
                    4: meter => { 5: inv_bat => { 6: bat, 7: bat } },
                    // A PV meter with two PV inverters.
                    8: meter => { 9: inv_pv, 10: inv_pv },
                },
            },
        }
        .build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #3, #9, #10)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([10, 3])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #3, #10)");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_pv },
                    4: meter => { 5: inv_bat => { 6: bat, 7: bat } },
                    8: meter => { 9: inv_pv, 10: inv_pv },
                },
                // A meter directly on the grid with three PV inverters.
                11: meter => { 12: inv_pv, 13: inv_pv, 14: inv_pv },
            },
        }
        .build(None)?;
        let formula = graph.pv_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #11, #3, #9, #10, #12, #13, #14)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([3, 9, 10, 12, 13])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #11, #3, #9, #10, #12, #13)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([3, 9, 10, 12, 13, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#2, #8, #11, #3, #9, #10, #12, #13, #14)");

        let formula = graph
            .pv_ac_coalesce_formula(Some(BTreeSet::from([10, 14])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#8, #11, #10, #14)");

        // Failure cases:
        let formula = graph.pv_ac_coalesce_formula(Some(BTreeSet::from([8])));
        assert_eq!(
            formula.unwrap_err().to_string(),
            "InvalidComponent: Component with id 8 is not a PV inverter."
        );

        Ok(())
    }

    /// A PV inverter that provides no telemetry is dropped from the coalesce,
    /// while the PV meter measuring it stays.
    #[test]
    fn test_pv_ac_coalesce_skips_no_telemetry() -> Result<(), Error> {
        let graph = graph! {
            0: grid => { 1: meter => { 2: meter => { 3: inv_pv, 4: inv_pv [ControlOnly] } } },
        }
        .build(None)?;
        // #4 is dropped; the PV meter #2 and the reporting inverter #3 remain.
        assert_eq!(
            graph.pv_ac_coalesce_formula(None)?.to_string(),
            "COALESCE(#2, #3)",
        );
        Ok(())
    }

    /// When every source component provides no telemetry, nothing is left to
    /// coalesce and the formula is `None`.
    #[test]
    fn test_pv_ac_coalesce_all_no_telemetry() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: inv_pv [ControlOnly] } }.build(None)?;
        assert_eq!(graph.pv_ac_coalesce_formula(None)?.to_string(), "None");
        Ok(())
    }
}
