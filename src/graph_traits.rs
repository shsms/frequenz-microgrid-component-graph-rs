// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the traits that need to be implemented by the types
//! that represent a node and an edge.

use crate::component_category::ComponentCategory;

/// This trait needs to be implemented by the type that represents a node.
pub trait Node {
    /// Returns the component id of the component.
    fn component_id(&self) -> u64;
    /// Returns the category of the category.
    fn category(&self) -> ComponentCategory;
    /// Returns true if the component can be read from and/or controlled.
    fn is_supported(&self) -> bool;
}

/// This trait needs to be implemented by the type that represents a connection.
pub trait Edge {
    /// Returns the source component id of the connection.
    fn source(&self) -> u64;
    /// Returns the destination component id of the connection.
    fn destination(&self) -> u64;
}
