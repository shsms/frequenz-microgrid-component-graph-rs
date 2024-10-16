// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

use crate::{component_category::CategoryPredicates, ComponentGraph, Edge, Error, Node};

impl<N, E> ComponentGraph<N, E>
where
    N: Node,
    E: Edge,
{
    pub(crate) fn has_successors(&self, component_id: u64) -> Result<bool, Error> {
        Ok(self.successors(component_id)?.next().is_some())
    }

    pub(crate) fn has_meter_successors(&self, component_id: u64) -> Result<bool, Error> {
        let mut has_successors = false;
        Ok(self.successors(component_id)?.any(|x| {
            has_successors = true;
            x.is_meter()
        }) && has_successors)
    }
}
