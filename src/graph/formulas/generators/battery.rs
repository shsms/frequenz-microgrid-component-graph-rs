// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating producer formulas.

use std::collections::BTreeSet;

use crate::component_category::CategoryPredicates;
use crate::graph::formulas::Formula;
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
        if self.inverter_ids.is_empty() {
            return Ok(Formula::new(Expr::number(0.0)));
        }

        aggregate(
            self.graph,
            self.inverter_ids.clone(),
            SourcePreference::prefer_meters(self.graph.config.prefer_meters_in_battery_formula()),
        )
        .map(Formula::new)
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

    use crate::{ComponentGraphConfig, Error, FormulaOverrides, graph::test_utils::graph};

    #[test]
    fn test_battery_formula() -> Result<(), Error> {
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

        let graph = graph! { 0: grid => { 1: meter } }.build(prefer_inverters_config.clone())?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(formula, "0.0");

        let graph = graph! {
            0: grid => {
                1: meter => {
                    // A battery meter with one inverter and one battery.
                    2: meter => { 3: inv_bat => { 4: bat } },
                },
            },
        }
        .build(prefer_inverters_config.clone())?;
        let formula = graph.battery_formula(None)?.to_string();
        assert_eq!(formula, "COALESCE(#3, #2, 0.0)");

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
        .build(prefer_inverters_config.clone())?;
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
        .build(prefer_inverters_config.clone())?;
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

        let g = graph! {
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
        };
        let graph = g.build(prefer_inverters_config)?;
        let graph_prefer_meters = g.build(Some(
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

        // As above: the component-first assertions hold only because the
        // per-battery override wins over the global meter-first setting.
        let graph = g.build(Some(
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
        let graph_prefer_meters = g.build(Some(
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
    #[test]
    fn test_battery_formula_finds_meter_through_passthrough() -> Result<(), Error> {
        let graph = graph! {
            0: grid => { 1: meter => { 2: transformer => { 3: inv_bat => { 4: bat } } } },
        }
        .build(None)?;
        let formula = graph.battery_formula(None)?.to_string();
        // Inverter falls back to its effective predecessor meter,
        // walking past the transformer.
        assert_eq!(formula, "COALESCE(#3, #1, 0.0)");
        Ok(())
    }

    /// A hybrid inverter is not a battery-power source: its AC reading
    /// mixes in its PV production. Selecting the battery explicitly must
    /// agree with the no-selection path, which already leaves hybrids out.
    #[test]
    fn test_battery_formula_ignores_a_hybrid_inverter() -> Result<(), Error> {
        let graph = graph! {
            0: grid => { 1: meter => { 2: inv_hybrid => { 3: bat } } },
        }
        .build(None)?;
        assert_eq!(graph.battery_formula(None)?.to_string(), "0.0");
        assert_eq!(
            graph
                .battery_formula(Some(BTreeSet::from([3])))?
                .to_string(),
            "0.0"
        );

        // Next to a real battery chain, both entry points count only that
        // chain.
        let graph = graph! {
            0: grid => {
                1: meter => {
                    2: inv_hybrid => { 3: bat },
                    4: meter => { 5: inv_bat => { 6: bat } },
                },
            },
        }
        .build(None)?;
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
