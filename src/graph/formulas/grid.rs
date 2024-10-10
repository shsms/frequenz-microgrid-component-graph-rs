// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the methods for generating grid formulas.

use super::expressions::FormulaExpression;
use crate::{ComponentGraph, Edge, Error, Node};

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    /// Generates the grid formula for the given node.
    ///
    /// The grid formula is the sum of all components connected to the grid.
    /// This formula can be used for calculating power or current metrics at the
    /// grid connection point.
    pub fn grid_formula(&self) -> Result<String, Error> {
        let mut components = vec![];
        for comp in self.successors(self.root_id)? {
            components.push(FormulaExpression::Component {
                component_id: comp.component_id(),
            });
        }
        Ok(FormulaExpression::Add { params: components }.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::test_utils::{TestComponent, TestConnection},
        ComponentCategory,
    };

    #[test]
    fn test_grid_formula() -> Result<(), Error> {
        let components = vec![
            TestComponent::new(1, ComponentCategory::Grid),
            TestComponent::new(2, ComponentCategory::Meter),
            TestComponent::new(3, ComponentCategory::Meter),
        ];
        let connections = vec![TestConnection::new(1, 2), TestConnection::new(1, 3)];

        let cg = ComponentGraph::try_new(components, connections)?;
        let formula = cg.grid_formula()?;
        assert_eq!(formula, "#3 + #2");

        Ok(())
    }
}
