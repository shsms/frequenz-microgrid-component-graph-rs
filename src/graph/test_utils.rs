// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the `TestComponent` and `TestConnection` types,
//! which implement the `Node` and `Edge` traits respectively.
//!
//! They are shared by all the test modules in the `graph` module.

use crate::{ComponentCategory, Edge, Node};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TestComponent(u64, ComponentCategory);

impl TestComponent {
    pub(super) fn new(id: u64, category: ComponentCategory) -> Self {
        TestComponent(id, category)
    }
}

impl Node for TestComponent {
    fn component_id(&self) -> u64 {
        self.0
    }

    fn category(&self) -> ComponentCategory {
        self.1.clone()
    }

    fn is_supported(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TestConnection(u64, u64);

impl TestConnection {
    pub(super) fn new(source: u64, destination: u64) -> Self {
        TestConnection(source, destination)
    }
}

impl Edge for TestConnection {
    fn source(&self) -> u64 {
        self.0
    }

    fn destination(&self) -> u64 {
        self.1
    }
}
