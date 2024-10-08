// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating consumption formulas.

use super::expressions::FormulaExpression as FExp;
use crate::{ComponentGraph, Edge, Error, Node};

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    fn consumption_components(&self, component_id: u64) -> Result<Vec<FExp>, Error> {
        if self.has_battery_successors(component_id)? {
            let mut components = vec![];
            for successor in self.successors(component_id)? {
                components.extend(self.consumption_components(successor.component_id())?);
            }
            return Ok(components);
        }
        Ok(vec![FExp::max(vec![
            FExp::number(0.0),
            FExp::component(component_id),
        ])])
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

    #[test]
    fn test_consumption_formula() {
        let components = vec![
            TestComponent::new(1, ComponentCategory::Grid),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Meter),
            TestComponent::new(4, ComponentCategory::Inverter(InverterType::Battery)),
            TestComponent::new(5, ComponentCategory::Battery(BatteryType::Unspecified)),
            TestComponent::new(6, ComponentCategory::Meter),
        ];
        let connections = vec![
            TestConnection::new(1, 2),
            TestConnection::new(1, 3),
            TestConnection::new(3, 4),
            TestConnection::new(4, 5),
            TestConnection::new(3, 6),
        ];

        let cg = ComponentGraph::try_new(components, connections).unwrap();
        let formula = cg.consumption_formula().unwrap();
        assert_eq!(formula, "#3 + #2");
    }
}
