// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Helper methods for checking invariants of a [`ComponentGraph`].

use crate::{ComponentCategory, Edge, Error, Node};

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

    /// Checks that the given node only has predecessors with the given categories.
    pub(super) fn ensure_predecessor_categories(
        &self,
        node: &N,
        categories: &[ComponentCategory],
    ) -> Result<(), Error> {
        for predecessor in self.cg.predecessors(node.component_id())? {
            if !categories.contains(&predecessor.category()) {
                return Err(Error::invalid_graph(format!(
                    "{}:{} can only have predecessors with categories: [{}]. Found {}:{}.",
                    node.category(),
                    node.component_id(),
                    categories
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    predecessor.category(),
                    predecessor.component_id()
                )));
            }
        }
        Ok(())
    }

    /// Checks that the given node only has successors with the given categories.
    pub(super) fn ensure_successor_categories(
        &self,
        node: &N,
        categories: &[ComponentCategory],
    ) -> Result<(), Error> {
        for successor in self.cg.successors(node.component_id())? {
            if !categories.contains(&successor.category()) {
                return Err(Error::invalid_graph(format!(
                    "{}:{} can only have successors with categories [{}]. Found {}:{}.",
                    node.category(),
                    node.component_id(),
                    categories
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    successor.category(),
                    successor.component_id()
                )));
            }
        }
        Ok(())
    }

    /// Checks that the given node does not have any successors with the given categories.
    pub(super) fn ensure_successor_not_categories(
        &self,
        node: &N,
        categories: &[ComponentCategory],
    ) -> Result<(), Error> {
        for successor in self.cg.successors(node.component_id())? {
            if categories.contains(&successor.category()) {
                return Err(Error::invalid_graph(format!(
                    "{}:{} can't have successors with categories [{}]. Found {}:{}.",
                    node.category(),
                    node.component_id(),
                    categories
                        .iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
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
