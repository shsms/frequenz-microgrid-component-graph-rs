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

        // Reject cycles before anything else: the remaining checks walk the
        // graph and would loop forever on a cyclic one. A detected cycle
        // short-circuits here, as does any internal failure during traversal.
        validator.validate_acyclicity(root, vec![])?;

        let mut errors = vec![];
        let mut fatal = false;

        if let Err(error) = validator.validate_connected_graph(root) {
            errors.extend(error.into_validation_errors()?);
            fatal |= !self.config.allow_unconnected_components;
        }

        for result in [
            validator.validate_root(),
            validator.validate_meters(),
            validator.validate_inverters(),
            validator.validate_batteries(),
            validator.validate_ev_chargers(),
            validator.validate_chps(),
            validator.validate_steam_boilers(),
        ] {
            if let Err(error) = result {
                errors.extend(error.into_validation_errors()?);
                fatal |= !self.config.allow_component_validation_failures;
            }
        }

        if errors.is_empty() {
            return Ok(());
        }

        let error = Error::validation_errors(errors);
        if fatal {
            Err(error)
        } else {
            // Every collected failure is tolerated by the configuration, so
            // report them as a warning instead of failing construction.
            tracing::warn!("{error}");
            Ok(())
        }
    }
}
