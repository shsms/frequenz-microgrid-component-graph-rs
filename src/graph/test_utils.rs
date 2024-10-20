// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module is only compiled when running unit tests and contains features
//! that are shared by all tests of the `graph` modue.
//!
//! - the `TestComponent` and `TestConnection` types, which implement the `Node`
//!   and `Edge` traits respectively.
//! - the `TestGraphBuilder`, which can declaratively build complex component
//!   graph configurations for use in tests.

use crate::{
    BatteryType, ComponentCategory, ComponentGraph, ComponentGraphConfig, Edge, Error,
    EvChargerType, InverterType, Node,
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

/// Represents a component added to the `ComponentGraphBuilder`.
#[derive(Eq, Hash, PartialEq, Copy, Clone)]
pub(super) struct ComponentHandle(u64);

impl ComponentHandle {
    /// Returns the component ID of the component.
    pub(super) fn component_id(&self) -> u64 {
        self.0
    }
}

/// A builder for creating complex component graph configurations easily, for
/// use in tests.
pub(super) struct ComponentGraphBuilder {
    components: Vec<TestComponent>,
    connections: Vec<TestConnection>,
    next_id: u64,
}

impl ComponentGraphBuilder {
    /// Creates a new `ComponentGraphBuilder`.
    pub(super) fn new() -> Self {
        let builder = ComponentGraphBuilder {
            components: Vec::new(),
            connections: Vec::new(),
            next_id: 0,
        };
        builder
    }

    /// Adds a component to the graph and returns its handle.
    pub(super) fn add_component(&mut self, category: ComponentCategory) -> ComponentHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.components
            .push(TestComponent::new(id, category.clone()));
        let handle = ComponentHandle(id);
        handle
    }

    /// Adds a grid component to the graph and returns its handle.
    pub(super) fn grid(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Grid)
    }

    /// Adds a meter to the graph and returns its handle.
    pub(super) fn meter(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Meter)
    }

    /// Adds a battery to the graph and returns its handle.
    pub(super) fn battery(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Battery(BatteryType::LiIon))
    }

    /// Adds a battery inverter to the graph and returns its handle.
    pub(super) fn battery_inverter(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Inverter(InverterType::Battery))
    }

    /// Adds a solar inverter to the graph and returns its handle.
    pub(super) fn solar_inverter(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Inverter(InverterType::Solar))
    }

    /// Adds an EV charger to the graph and returns its handle.
    pub(super) fn ev_charger(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::EvCharger(EvChargerType::Ac))
    }

    /// Adds a CHP to the graph and returns its handle.
    pub(super) fn chp(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Chp)
    }

    /// Connects two components in the graph.
    pub(super) fn connect(&mut self, from: ComponentHandle, to: ComponentHandle) -> &mut Self {
        self.connections.push(TestConnection::new(from.0, to.0));
        self
    }

    /// Adds a meter, followed by the given number of inverters and batteries,
    /// and returns a handle to the meter.
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

    /// Adds a battery inverter, followed by the given number of batteries,
    /// and returns a handle to the battery inverter.
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

    /// Adds a meter, followed by the given number of PV inverters, and returns a
    /// handle to the meter.
    pub(super) fn meter_pv_chain(&mut self, num_inverters: usize) -> ComponentHandle {
        let meter = self.meter();
        for _ in 0..num_inverters {
            let inverter = self.solar_inverter();
            self.connect(meter, inverter);
        }
        meter
    }

    /// Adds a meter, followed by the given number of CHPs, and returns a
    /// handle to the meter.
    pub(super) fn meter_chp_chain(&mut self, num_chp: usize) -> ComponentHandle {
        let meter = self.meter();
        for _ in 0..num_chp {
            let chp = self.chp();
            self.connect(meter, chp);
        }
        meter
    }

    /// Adds a meter, followed by the given number of EV chargers, and returns a
    /// handle to the meter.
    pub(super) fn meter_ev_charger_chain(&mut self, num_ev_chargers: usize) -> ComponentHandle {
        let meter = self.meter();
        for _ in 0..num_ev_chargers {
            let ev_charger = self.ev_charger();
            self.connect(meter, ev_charger);
        }
        meter
    }

    /// Builds and returns the component graph from the components and
    /// connections added to the builder.
    pub(super) fn build(
        &self,
        config: Option<ComponentGraphConfig>,
    ) -> Result<ComponentGraph<TestComponent, TestConnection>, Error> {
        ComponentGraph::try_new(
            self.components.clone(),
            self.connections.clone(),
            config.unwrap_or_default(),
        )
    }
}
