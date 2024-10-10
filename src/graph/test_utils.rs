// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module is only compiled when running unit tests and contains features
//! that are shared by all tests of the `graph` modue.
//!
//! - the `TestComponent` and `TestConnection` types, which implement the `Node`
//!   and `Edge` traits respectively.
//! - the `TestGraphBuilder`, which can declaratively build complicated
//!   component configurations for use in tests.

use std::collections::HashMap;

use crate::{
    BatteryType, ComponentCategory, ComponentGraph, Edge, Error, EvChargerType, InverterType, Node,
};

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

#[derive(Eq, Hash, PartialEq, Copy, Clone)]
pub(super) struct ComponentHandle(u64);

pub(super) struct ComponentGraphBuilder {
    components: Vec<TestComponent>,
    connections: Vec<TestConnection>,
    next_id: u64,
    category_map: HashMap<ComponentHandle, ComponentCategory>,
}

impl ComponentGraphBuilder {
    /// Creates a new `ComponentGraphBuilder`
    pub(super) fn new() -> Self {
        let builder = ComponentGraphBuilder {
            components: Vec::new(),
            connections: Vec::new(),
            next_id: 0,
            category_map: HashMap::new(),
        };
        builder
    }

    /// Adds a component to the graph and returns its handle
    pub(super) fn add_component(&mut self, category: ComponentCategory) -> ComponentHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.components
            .push(TestComponent::new(id, category.clone()));
        let handle = ComponentHandle(id);
        self.category_map.insert(handle, category);
        handle
    }

    pub(super) fn grid(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Grid)
    }

    pub(super) fn meter(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Meter)
    }

    pub(super) fn battery(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Battery(BatteryType::LiIon))
    }

    pub(super) fn battery_inverter(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Inverter(InverterType::Battery))
    }

    pub(super) fn solar_inverter(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Inverter(InverterType::Solar))
    }

    pub(super) fn ev_charger(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::EvCharger(EvChargerType::Ac))
    }

    /// Connects two components in the graph
    pub(super) fn connect(&mut self, from: ComponentHandle, to: ComponentHandle) -> &mut Self {
        self.connections.push(TestConnection::new(from.0, to.0));
        self
    }

    pub(super) fn meter_bat_chain(
        &mut self,
        num_inverters: usize,
        num_batteries: usize,
    ) -> ComponentHandle {
        let meter = self.meter();
        let mut inverters = vec![];
        for _ in 0..num_inverters {
            let inverter = self.battery_inverter();
            self.connect(meter, inverter);
            inverters.push(inverter);
        }
        for _ in 0..num_batteries {
            let battery = self.battery();
            for inverter in &inverters {
                self.connect(*inverter, battery);
            }
        }
        meter
    }

    pub(super) fn inv_bat_chain(&mut self, num_batteries: usize) -> ComponentHandle {
        let inverter = self.battery_inverter();
        let mut batteries = vec![];
        for _ in 0..num_batteries {
            let battery = self.battery();
            batteries.push(battery);
        }
        for battery in &batteries {
            self.connect(inverter, *battery);
        }
        inverter
    }

    pub(super) fn meter_pv_chain(&mut self, num_inverters: usize) -> ComponentHandle {
        let meter = self.meter();
        for _ in 0..num_inverters {
            let inverter = self.solar_inverter();
            self.connect(meter, inverter);
        }
        meter
    }

    /// Finalizes the graph and returns the components and connections
    pub(super) fn build(&self) -> Result<ComponentGraph<TestComponent, TestConnection>, Error> {
        ComponentGraph::try_new(self.components.clone(), self.connections.clone())
    }
}
