// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating consumption formulas.

use super::{expressions::FormulaExpression as FExp, traversal::FindFallback};
use crate::{component_category::CategoryPredicates, ComponentGraph, Edge, Error, Node};

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    fn consumption_components(&self, component_id: u64) -> Result<Vec<FExp>, Error> {
        if self.has_battery_successors(component_id)? {
            let mut components = vec![];
            for successor in self.successors(component_id)? {
                if successor.is_meter() {
                    components.extend(self.consumption_components(successor.component_id())?);
                }
            }
            return Ok(components);
        }
        Ok(vec![FindFallback {
            prefer_meters: true,
            wrap_method: |exp| {
                if matches!(exp, FExp::Component { .. }) {
                    FExp::max(vec![FExp::number(0.0), exp])
                } else {
                    exp
                }
            },
            category_predicate: |node: &N| node.is_meter(),
            graph: self,
        }
        .with_fallback(vec![component_id])?])
    }

    /// Generates the consumption formula for the given node.
    pub fn consumption_formula(&self) -> Result<String, Error> {
        self.consumption_components(self.root_id)
            .map(|params| FExp::Add { params }.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::test_types::{TestComponent, TestConnection},
        BatteryType, ComponentCategory, InverterType,
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
        let formula = cg.consumption_formula().unwrap();
        assert_eq!(formula, "#3 + #2");
    }
}
