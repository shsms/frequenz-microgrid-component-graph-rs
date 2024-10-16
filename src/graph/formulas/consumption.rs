// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating consumption formulas.

use super::{expressions::FormulaExpression, traversal::FindFallback, FormulaBuilder};
use crate::{component_category::CategoryPredicates, Edge, Error, Node};

impl<N, E> FormulaBuilder<'_, N, E>
where
    N: Node,
    E: Edge,
{
    fn consumption_non_component_meters(
        &self,
        component_id: u64,
        wrap_method: Option<fn(FormulaExpression) -> FormulaExpression>,
    ) -> Result<Option<FormulaExpression>, Error> {
        let component = self.graph.component(component_id)?;
        if component.is_meter() && !self.graph.is_component_meter(component_id)? {
            let mut expr = FormulaExpression::from(component);
            if let Some(successors) = self.successor_meters(component_id)? {
                expr = expr + successors;
            } else if let Some(wrap_method) = wrap_method {
                expr = wrap_method(expr);
            }
            Ok(Some(expr))
        } else {
            Ok(None)
        }
    }

    fn successor_meters(&self, component_id: u64) -> Result<Option<FormulaExpression>, Error> {
        let mut expr = None;
        for successor in self.graph.successors(component_id)? {
            expr = Some(match expr {
                Some(expr) => expr - self.meters_with_fallback(successor, None)?,
                None => -self.meters_with_fallback(successor, None)?,
            });
            // if successor.is_meter() && !self.graph.is_component_meter(successor.component_id())? {
            //     if let Some(successors) =
            //         self.non_component_meter_successors(successor.component_id())?
            //     {
            //         expr = Some(match expr {
            //             Some(expr) => expr + successors,
            //             None => successors,
            //         });
            //     }
            // } else {
            //     expr = Some(match expr {
            //         Some(expr) => expr - self.meters_with_fallback(successor, None)?,
            //         None => -self.meters_with_fallback(successor, None)?,
            //     });
            // }
        }

        Ok(expr)
    }

    fn consumption_component_meters(
        &self,
        component_id: u64,
        wrap_method: Option<fn(FormulaExpression) -> FormulaExpression>,
    ) -> Result<Option<FormulaExpression>, Error> {
        let mut expr = None;
        for component_meter in self.graph.find_all(
            component_id,
            |n| {
                self.graph
                    .is_component_meter(n.component_id())
                    .unwrap_or(false)
            },
            petgraph::EdgeDirection::Outgoing,
        )? {
            if self
                .graph
                .is_battery_meter(component_meter.component_id())?
            {
                continue;
            }
            // let meter_expr = if let Some(wrap_method) = wrap_method {
            //     wrap_method(component_meter.into())
            // } else {
            //     component_meter.into()
            // };
            expr = Some(match expr {
                // Some(expr) => expr + meter_expr,
                // None => meter_expr,
                Some(expr) => expr + self.meters_with_fallback(component_meter, wrap_method)?,
                None => self.meters_with_fallback(component_meter, wrap_method)?,
            });
        }
        Ok(expr)
    }

    fn meters_with_fallback(
        &self,
        successor: &N,
        wrap_method: Option<fn(FormulaExpression) -> FormulaExpression>,
    ) -> Result<FormulaExpression, Error> {
        if successor.is_meter() {
            Ok(FindFallback {
                prefer_meters: true,
                only_single_component_category_meters: false,
                wrap_method,
                graph: self.graph,
            }
            .with_fallback(vec![successor.component_id()])?)
        } else {
            Ok(FormulaExpression::from(successor))
        }
    }

    /// Generates the consumption formula for the given node.
    pub fn consumption_formula(&self) -> Result<String, Error> {
        let wrap_method = |exp| {
            if matches!(exp, FormulaExpression::Component { .. }) {
                FormulaExpression::max(vec![FormulaExpression::number(0.0), exp])
            } else {
                exp
            }
        };
        if let Some(successors) = self
            .graph
            .successors(self.graph.root_id)?
            .map(|s| self.consumption_non_component_meters(s.component_id(), Some(wrap_method)))
            .reduce(|a, b| match (a?, b?) {
                (Some(a), Some(b)) => Ok(Some(a + b)),
                (None, a) | (a, None) => Ok(a),
            })
            .map(|v| {
                match (
                    v?,
                    self.consumption_component_meters(self.graph.root_id, Some(wrap_method))?,
                ) {
                    (Some(a), Some(b)) => Ok(Some(a + b)),
                    (None, a) | (a, None) => Ok(a),
                }
            })
        {
            Ok(successors.map(|s| s.map(|s| s.to_string()).unwrap_or_default())?)
        } else {
            Ok(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::test_utils::{ComponentGraphBuilder, TestComponent, TestConnection},
        BatteryType, ComponentCategory, ComponentGraph, InverterType,
    };

    fn nodes_and_edges() -> (Vec<TestComponent>, Vec<TestConnection>) {
        let components = vec![
            TestComponent::new(1, ComponentCategory::Grid),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Meter),
            TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::NaIon)),
            TestComponent::new(6, ComponentCategory::Meter),
            TestComponent::new(7, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(8, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(9, ComponentCategory::Meter),
            TestComponent::new(10, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent::new(11, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent::new(12, ComponentCategory::Meter),
            TestComponent::new(13, ComponentCategory::Chp),
            TestComponent::new(14, ComponentCategory::Meter),
            TestComponent::new(15, ComponentCategory::Chp),
            TestComponent::new(16, ComponentCategory::Inverter(InverterType::Solar)),
            TestComponent::new(17, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(18, ComponentCategory::Battery(BatteryType::LiIon)),
        ];
        let connections = vec![
            // Single Grid meter
            TestConnection::new(1, 2),
            // Battery chain
            TestConnection::new(2, 3),
            TestConnection::new(3, 4),
            TestConnection::new(4, 5),
            // Battery chain
            TestConnection::new(2, 6),
            TestConnection::new(6, 7),
            TestConnection::new(7, 8),
            // Solar chain
            TestConnection::new(2, 9),
            TestConnection::new(9, 10),
            TestConnection::new(9, 11),
            // CHP chain
            TestConnection::new(2, 12),
            TestConnection::new(12, 13),
            // Mixed chain
            TestConnection::new(2, 14),
            TestConnection::new(14, 15),
            TestConnection::new(14, 16),
            TestConnection::new(14, 17),
            TestConnection::new(17, 18),
        ];

        (components, connections)
    }

    #[test]
    fn test_consumption_formula() {
        let (components, connections) = nodes_and_edges();

        let cg = ComponentGraph::try_new(components, connections).unwrap();
        let builder = FormulaBuilder { graph: &cg };
        let formula = builder.consumption_formula().unwrap();
        assert_eq!(
            formula,
            concat!(
                "#2 - (",
                "COALESCE(#14, #17 + #16 + #15) + COALESCE(#12, #13) + ",
                "COALESCE(#9, #11 + #10) + COALESCE(#6, #7) + COALESCE(#3, #4)",
                ") + ",
                "COALESCE(MAX(0, #9), MAX(0, #11) + MAX(0, #10)) + ",
                "COALESCE(MAX(0, #12), MAX(0, #13))",
                // "MAX(0, #9) + MAX(0, #12)",
            )
        );
    }

    #[test]
    fn test_consumption_formula_with_grid_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        // Add a single dangling meter to the grid.
        let grid_meter = builder.meter();
        builder.connect(grid, grid_meter);

        let graph = builder.build()?;
        let formula = FormulaBuilder::new(&graph).consumption_formula()?;
        assert_eq!(formula, "MAX(0, #1)");

        // Add a battery meter with one battery inverter and one battery to the
        // grid meter.
        let meter_bat_chain = builder.meter_bat_chain(1, 1);
        builder.connect(grid_meter, meter_bat_chain);

        let graph = builder.build()?;
        let formula = FormulaBuilder::new(&graph).consumption_formula()?;
        // Formula subtracts the battery power from the grid meter.
        assert_eq!(formula, "#1 - COALESCE(#2, #3)");

        // Add a solar meter with two solar inverters to the grid meter.
        let meter_pv_chain = builder.meter_pv_chain(2);
        builder.connect(grid_meter, meter_pv_chain);

        let graph = builder.build()?;
        let formula = FormulaBuilder::new(&graph).consumption_formula()?;
        assert_eq!(
            formula,
            concat!(
                // subtracts solar and battery powers from the grid meter.
                "#1 - (COALESCE(#5, #7 + #6) + COALESCE(#2, #3)) + ",
                // any measured consumption from the solar inverters, or the sum of
                // the solar inverters, if meter data is not available.
                "COALESCE(MAX(0, #5), MAX(0, #7) + MAX(0, #6))"
            )
        );

        // Add a "mixed" meter with a CHP, an ev charger and a solar inverter to
        // the grid meter.
        let solar_inverter = builder.solar_inverter();
        let chp = builder.chp();
        let ev_charger = builder.ev_charger();
        let meter = builder.meter();
        builder.connect(meter, solar_inverter);
        builder.connect(meter, chp);
        builder.connect(meter, ev_charger);
        builder.connect(grid_meter, meter);

        let graph = builder.build()?;
        let formula = FormulaBuilder::new(&graph).consumption_formula()?;
        assert_eq!(
            formula,
            concat!(
                // subtracts the solar, battery, CHP and EV charger powers from
                // the grid meter.  This allows unspecified consumption of the
                // "mixed" meter to be accounted for as well.
                "#1 - ",
                "(COALESCE(#11, #10 + #9 + #8) + COALESCE(#5, #7 + #6) + COALESCE(#2, #3)) + ",
                // No changes here.
                "COALESCE(MAX(0, #5), MAX(0, #7) + MAX(0, #6))"
            )
        );

        Ok(())
    }

    #[test]
    fn test_consumption_formula_without_grid_meter() -> Result<(), Error> {
        let mut builder = ComponentGraphBuilder::new();
        let grid = builder.grid();

        let meter_bat_chain = builder.meter_bat_chain(1, 2);
        builder.connect(grid, meter_bat_chain);

        let graph = builder.build()?;
        let formula = graph.consumer_formula()?;
        assert_eq!(formula, "");

        Ok(())
    }
}
