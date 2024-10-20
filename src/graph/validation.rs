// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for validating a [`ComponentGraph`].

mod invariant_checks;
mod validate_graph;
mod validate_neighbors;

use crate::{ComponentGraph, Edge, Error, Node};

pub(crate) struct ComponentGraphValidator<'a, N, E>
where
    N: Node,
    E: Edge,
{
    cg: &'a ComponentGraph<N, E>,
    root: &'a N,
}

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let Ok(root) = self.component(self.root_id) else {
            return Err(Error::internal(format!(
                "Grid component not found with detected component ID: {}.",
                self.root_id
            )));
        };

        let validator = ComponentGraphValidator { cg: self, root };

        // Fail immediately if there are cycles in the graph, as this may cause
        // subsequent validations to get stuck in an infinite loop.
        validator.validate_acyclicity(root, vec![])?;

        let mut errors = vec![];
        for result in [
            validator.validate_connected_graph(root),
            validator.validate_root(),
            validator.validate_meters(),
            validator.validate_inverters(),
            validator.validate_batteries(),
            validator.validate_ev_chargers(),
            validator.validate_chps(),
        ] {
            if let Err(e) = result {
                errors.push(e);
            }
        }
        if errors.len() == 1 {
            return Err(errors[0].clone());
        } else if !errors.is_empty() {
            let error_messages = "Multiple validation failures:\n    ".to_string()
                + &errors
                    .into_iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n    ");
            return Err(Error::invalid_graph(error_messages));
        }
        Ok(())
    }
}
