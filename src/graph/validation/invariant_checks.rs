// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Helper methods for checking invariants of a [`ComponentGraph`].

use crate::{Edge, Error, Node};

use super::ComponentGraphValidator;

impl<N, E> ComponentGraphValidator<'_, N, E>
where
    N: Node,
    E: Edge,
{
    /// Checks that the given node is a leaf node.
    pub(super) fn ensure_leaf(&self, node: &N) -> Result<(), Error> {
        if let Some(successor) = self.cg.successors(node.component_id())?.next() {
            return Err(Error::invalid_graph(format!(
                "{}:{} can't have any successors. Found {}:{}.",
                node.category(),
                node.component_id(),
                successor.category(),
                successor.component_id()
            )));
        }
        Ok(())
    }

    /// Checks that the given node is *not* a leaf node.
    pub(super) fn ensure_not_leaf(&self, node: &N) -> Result<(), Error> {
        if self.cg.successors(node.component_id())?.next().is_none() {
            return Err(Error::invalid_graph(format!(
                "{}:{} must have at least one successor.",
                node.category(),
                node.component_id()
            )));
        }
        Ok(())
    }

    /// Checks that the given node is a root node.
    pub(super) fn ensure_root(&self, node: &N) -> Result<(), Error> {
        if let Some(predecessor) = self.cg.predecessors(node.component_id())?.next() {
            return Err(Error::invalid_graph(format!(
                "{}:{} can't have any predecessors. Found {}:{}.",
                node.category(),
                node.component_id(),
                predecessor.category(),
                predecessor.component_id()
            )));
        }
        Ok(())
    }

    /// Checks that the given predicate holds for all predecessors of the given node.
    pub(super) fn ensure_on_predecessors(
        &self,
        node: &N,
        predicate: impl Fn(&N) -> bool,
        failure_message: &str,
    ) -> Result<(), Error> {
        for predecessor in self.cg.predecessors(node.component_id())? {
            if !predicate(predecessor) {
                return Err(Error::invalid_graph(format!(
                    "{}:{} can only have predecessors that are {}. Found {}:{}.",
                    node.category(),
                    node.component_id(),
                    failure_message,
                    predecessor.category(),
                    predecessor.component_id()
                )));
            }
        }
        Ok(())
    }

    /// Checks that the given predicate holds for all successors of the given node.
    pub(super) fn ensure_on_successors(
        &self,
        node: &N,
        predicate: impl Fn(&N) -> bool,
        failure_message: &str,
    ) -> Result<(), Error> {
        for successor in self.cg.successors(node.component_id())? {
            if !predicate(successor) {
                return Err(Error::invalid_graph(format!(
                    "{}:{} can only have successors that are {}. Found {}:{}.",
                    node.category(),
                    node.component_id(),
                    failure_message,
                    successor.category(),
                    successor.component_id()
                )));
            }
        }
        Ok(())
    }

    /// Checks that the given node's successors are exclusive to it.
    ///
    /// A node's successors are exclusive to the node if they don't have any
    /// other predecessors.
    pub(super) fn ensure_exclusive_successors(&self, node: &N) -> Result<(), Error> {
        for successor in self.cg.successors(node.component_id())? {
            if self.cg.predecessors(successor.component_id())?.count() > 1 {
                return Err(Error::invalid_graph(format!(
                    "{}:{} can't have successors with multiple predecessors. Found {}:{}.",
                    node.category(),
                    node.component_id(),
                    successor.category(),
                    successor.component_id()
                )));
            }
        }
        Ok(())
    }
}
