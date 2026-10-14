// License: MIT
// Copyright © 2025 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating battery AC coalesce formulas.

use crate::component_category::CategoryPredicates;
use std::collections::BTreeSet;

use crate::{ComponentGraph, Edge, Error, Node, graph::formulas::Formula};

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
    ///
    /// A component that provides no telemetry is skipped. When no component
    /// provides telemetry, the formula is `None`.
    pub fn build(self) -> Result<Formula, Error> {
        let mut meters: BTreeSet<u64> = BTreeSet::new();

        for inv_id in &self.inverter_ids {
            for pred in self.graph.predecessors(*inv_id)? {
                if self.graph.is_battery_meter(pred.component_id())? {
                    meters.insert(pred.component_id());
                }
            }
        }
        let coalesced = super::coalesce_with_telemetry(
            self.graph,
            meters.into_iter().chain(self.inverter_ids),
        )?;

        Ok(Formula::new(coalesced))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{ComponentGraphConfig, Error, graph::test_utils::graph};

    #[test]
    fn test_battery_ac_voltage_formula() -> Result<(), Error> {
        let graph = graph! { 0: grid => { 1: meter } }.build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "None");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    // A battery meter with one inverter and one battery.
                    2: meter => { 3: inv_bat => { 4: bat } },
                },
            },
        }
        .build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #3)");
        let formula = graph.battery_ac_coalesce_formula(Some(BTreeSet::from([12])));
        assert_eq!(
            formula,
            Err(Error::component_not_found(
                "Component with id 12 not found."
            ))
        );

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_bat => { 4: bat } },
                    // A second battery meter with one inverter and two
                    // batteries.
                    5: meter => { 6: inv_bat => { 7: bat, 8: bat } },
                },
            },
        }
        .build(None)?;
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

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_bat => { 4: bat } },
                    5: meter => { 6: inv_bat => { 7: bat, 8: bat } },
                    // A third battery meter with two inverters with two
                    // connected batteries.
                    9: meter => {
                        10: inv_bat => { 12: bat, 13: bat },
                        11: inv_bat => { 12, 13 },
                    },
                },
            },
        }
        .build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #5, #9, #3, #6, #10, #11)");

        let formula = graph
            .battery_ac_coalesce_formula(Some(BTreeSet::from([12, 13])))?
            .to_string();
        assert_eq!(formula, "COALESCE(#9, #10, #11)");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_bat => { 4: bat } },
                    5: meter => { 6: inv_bat => { 7: bat, 8: bat } },
                    9: meter => {
                        10: inv_bat => { 12: bat, 13: bat },
                        11: inv_bat => { 12, 13 },
                    },
                    // A PV meter with two PV inverters.
                    14: meter => { 15: inv_pv, 16: inv_pv },
                },
            },
        }
        .build(None)?;
        let formula = graph.battery_ac_coalesce_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#2, #5, #9, #3, #6, #10, #11)");

        let g = graph! {
            0: grid => {
                1: meter => {
                    2: meter => { 3: inv_bat => { 4: bat } },
                    5: meter => { 6: inv_bat => { 7: bat, 8: bat } },
                    9: meter => {
                        10: inv_bat => { 12: bat, 13: bat },
                        11: inv_bat => { 12, 13 },
                    },
                    14: meter => { 15: inv_pv, 16: inv_pv },
                },
                // A battery meter with two inverters that have their own
                // batteries.
                17: meter => {
                    18: inv_bat => { 19: bat },
                    20: inv(Unspecified) => { 21: bat },
                },
            },
        };

        assert!(
            g.build(None).is_err_and(|x| x.to_string()
                == "InvalidComponent: InverterType not specified for inverter: 20")
        );

        let graph = g.build(Some(
            ComponentGraphConfig::builder()
                .allow_unspecified_inverters(true)
                .build(),
        ))?;
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

    /// A battery inverter that provides no telemetry is dropped from the
    /// coalesce, while the battery meter measuring it stays.
    #[test]
    fn test_battery_ac_coalesce_skips_no_telemetry() -> Result<(), Error> {
        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: meter => {
                        3: inv_bat => { 5: bat },
                        4: inv_bat [ControlOnly] => { 5 },
                    },
                },
            },
        }
        .build(None)?;
        // #4 is dropped; the battery meter #2 and the reporting inverter #3 remain.
        assert_eq!(
            graph.battery_ac_coalesce_formula(None)?.to_string(),
            "COALESCE(#2, #3)",
        );
        Ok(())
    }

    /// A hybrid inverter is not a battery-chain voltage source either:
    /// selecting its battery explicitly must agree with the no-selection
    /// path, which leaves hybrids out.
    #[test]
    fn test_battery_ac_voltage_formula_ignores_a_hybrid_inverter() -> Result<(), Error> {
        let graph =
            graph! { 0: grid => { 1: meter => { 2: inv_hybrid => { 3: bat } } } }.build(None)?;
        assert_eq!(graph.battery_ac_coalesce_formula(None)?.to_string(), "None");
        assert_eq!(
            graph
                .battery_ac_coalesce_formula(Some(BTreeSet::from([3])))?
                .to_string(),
            "None"
        );

        Ok(())
    }
}
