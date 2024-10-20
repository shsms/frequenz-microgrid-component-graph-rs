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
        let mut validation_failed = false;

        if let Err(err) = validator.validate_connected_graph(root) {
            errors.push(err);
            validation_failed = !self.config.allow_unconnected_components;
        }

        for result in [
            validator.validate_root(),
            validator.validate_meters(),
            validator.validate_inverters(),
            validator.validate_batteries(),
            validator.validate_ev_chargers(),
            validator.validate_chps(),
        ] {
            if let Err(e) = result {
                errors.push(e);
                validation_failed = !self.config.allow_component_validation_failures;
            }
        }
        match errors.len() {
            0 => {}
            1 => {
                if validation_failed {
                    return Err(errors[0].clone());
                } else {
                    tracing::warn!("{}", errors[0]);
                }
            }
            _ => {
                let err = Error::invalid_graph(format!(
                    "Multiple validation failures:\n    {}",
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n    ")
                ));
                if validation_failed {
                    return Err(err);
                } else {
                    tracing::warn!("{}", err);
                }
            }
        }
        Ok(())
    }
}
