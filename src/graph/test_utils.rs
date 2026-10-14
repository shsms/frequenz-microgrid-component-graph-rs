// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module is only compiled when running unit tests and contains features
//! that are shared by all tests of the `graph` module.
//!
//! - the `TestComponent` and `TestConnection` types, which implement the `Node`
//!   and `Edge` traits respectively.
//! - the `graph!` macro and the `TestGraph` it produces, which declare
//!   component graph configurations for use in tests.

use crate::{
    BatteryType, ComponentCategory, ComponentGraph, ComponentGraphConfig, Edge, Error,
    EvChargerType, InverterType, Node, OperationalMode, ValidationError,
};

/// Builds the [`Error`] that validation returns when it collects a single
/// failure with the given message and affected component IDs.
pub(super) fn validation_error(message: &str, component_ids: &[u64]) -> Error {
    Error::validation_errors(vec![ValidationError::new(message, component_ids.to_vec())])
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TestComponent(u64, ComponentCategory, OperationalMode);

impl TestComponent {
    pub(super) fn new(id: u64, category: ComponentCategory) -> Self {
        TestComponent(id, category, OperationalMode::Unspecified)
    }
}

impl Node for TestComponent {
    fn component_id(&self) -> u64 {
        self.0
    }

    fn category(&self) -> ComponentCategory {
        self.1
    }

    fn operational_mode(&self) -> OperationalMode {
        self.2
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

/// A component graph declared with [`graph!`], for use in tests.
///
/// Holds components and connections in declaration order.
#[derive(Default)]
pub(super) struct TestGraph {
    components: Vec<TestComponent>,
    connections: Vec<TestConnection>,
}

impl TestGraph {
    pub(super) fn component(
        &mut self,
        id: u64,
        category: ComponentCategory,
        mode: OperationalMode,
    ) {
        self.components.push(TestComponent(id, category, mode));
    }

    pub(super) fn connect(&mut self, source: u64, destination: u64) {
        self.connections.push(TestConnection(source, destination));
    }

    /// Returns the components in declaration order.
    pub(super) fn components(&self) -> &[TestComponent] {
        &self.components
    }

    /// Returns the connections in declaration order.
    pub(super) fn connections(&self) -> &[TestConnection] {
        &self.connections
    }

    /// Builds the component graph.
    ///
    /// Inserts components sorted by ID. The sort is stable, so components with
    /// the same ID keep their declaration order. Inserts connections in
    /// declaration order.
    pub(super) fn build(
        &self,
        config: Option<ComponentGraphConfig>,
    ) -> Result<ComponentGraph<TestComponent, TestConnection>, Error> {
        let mut components = self.components.clone();
        components.sort_by_key(|c| c.0);
        ComponentGraph::try_new(
            components,
            self.connections.clone(),
            config.unwrap_or_default(),
        )
    }
}

/// Declares a [`TestGraph`] as a forest of component trees.
///
/// ```text
/// graph! { node, node, ... }
/// node     := ID ':' kind ('(' Subtype ')')? ('[' Mode ']')? children?
///           | ID children?
/// children := '=>' '{' node, ... '}'
/// ```
///
/// `kind` is one of the `@kind` rules below. Only `bat`, `inv` and `ev` take a
/// `(Subtype)`, and `inv` needs one.
///
/// `ID: kind` declares a component; a bare `ID` refers to one declared
/// elsewhere and only adds edges. Every child gets an edge from its parent.
/// Nothing is validated here; [`TestGraph::build`] leaves that to
/// [`ComponentGraph::try_new`], so invalid graphs can be declared too.
///
/// Input that does not fit the grammar, such as `graph! { 0 grid }`, fails to
/// compile with a `graph!: cannot parse node: …` error.
///
/// ```text
/// graph! {
///     0: grid => {
///         1: meter => {
///             2: inv_bat => { 4: bat, 5: bat },
///             3: inv_bat => { 4, 5 },
///         },
///         6: meter [Inactive],
///     },
/// }
/// ```
macro_rules! graph {
    (@kind grid) => {
        $crate::ComponentCategory::GridConnectionPoint
    };
    (@kind meter) => {
        $crate::ComponentCategory::Meter
    };
    (@kind bat) => {
        $crate::ComponentCategory::Battery($crate::BatteryType::LiIon)
    };
    (@kind bat($t:ident)) => {
        $crate::ComponentCategory::Battery($crate::BatteryType::$t)
    };
    (@kind inv_bat) => {
        $crate::ComponentCategory::Inverter($crate::InverterType::Battery)
    };
    (@kind inv_pv) => {
        $crate::ComponentCategory::Inverter($crate::InverterType::Pv)
    };
    (@kind inv_hybrid) => {
        $crate::ComponentCategory::Inverter($crate::InverterType::Hybrid)
    };
    (@kind inv($t:ident)) => {
        $crate::ComponentCategory::Inverter($crate::InverterType::$t)
    };
    (@kind ev) => {
        $crate::ComponentCategory::EvCharger($crate::EvChargerType::Ac)
    };
    (@kind ev($t:ident)) => {
        $crate::ComponentCategory::EvCharger($crate::EvChargerType::$t)
    };
    (@kind chp) => {
        $crate::ComponentCategory::Chp
    };
    (@kind wind) => {
        $crate::ComponentCategory::WindTurbine
    };
    (@kind boiler) => {
        $crate::ComponentCategory::SteamBoiler
    };
    (@kind transformer) => {
        $crate::ComponentCategory::PowerTransformer
    };
    (@kind unspecified) => {
        $crate::ComponentCategory::Unspecified
    };
    (@kind $($unknown:tt)*) => {
        compile_error!(concat!("graph!: unknown kind: ", stringify!($($unknown)*)))
    };
    (@mode) => {
        $crate::OperationalMode::Unspecified
    };
    (@mode $m:ident) => {
        $crate::OperationalMode::$m
    };
    (@nodes $g:ident [$($parent:literal)?];) => {};
    (@nodes $g:ident [$($parent:literal)?];
        $id:literal $(: $kind:ident $(($sub:ident))? $([$mode:ident])?)?
        $(=> { $($children:tt)* })? $(, $($rest:tt)*)?
    ) => {
        $($g.component(
            $id,
            $crate::graph::test_utils::graph!(@kind $kind $(($sub))?),
            $crate::graph::test_utils::graph!(@mode $($mode)?),
        );)?
        $($g.connect($parent, $id);)?
        $($crate::graph::test_utils::graph!(@nodes $g [$id]; $($children)*);)?
        $crate::graph::test_utils::graph!(@nodes $g [$($parent)?]; $($($rest)*)?);
    };
    (@nodes $g:ident [$($parent:literal)?]; $($unparsed:tt)*) => {
        compile_error!(concat!("graph!: cannot parse node: ", stringify!($($unparsed)*)))
    };
    () => { $crate::graph::test_utils::TestGraph::default() };
    ($($body:tt)+) => {{
        let mut g = $crate::graph::test_utils::TestGraph::default();
        $crate::graph::test_utils::graph!(@nodes g []; $($body)+);
        g
    }};
}
pub(super) use graph;

/// Represents a component added to the `ComponentGraphBuilder`.
#[derive(Eq, Hash, PartialEq, Copy, Clone)]
pub(super) struct ComponentHandle(u64);

impl ComponentHandle {
    pub(super) fn new(id: u64) -> Self {
        ComponentHandle(id)
    }

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
        ComponentGraphBuilder {
            components: Vec::new(),
            connections: Vec::new(),
            next_id: 0,
        }
    }

    /// Adds a component to the graph and returns its handle.
    pub(super) fn add_component(&mut self, category: ComponentCategory) -> ComponentHandle {
        self.add_component_with_mode(category, OperationalMode::Unspecified)
    }

    /// Adds a component with the given id to the graph and returns its handle.
    pub(super) fn add_component_with_id(
        &mut self,
        id: u64,
        category: ComponentCategory,
    ) -> ComponentHandle {
        self.components.push(TestComponent::new(id, category));

        ComponentHandle(id)
    }

    /// Adds a component with the given operational mode to the graph and returns
    /// its handle.
    pub(super) fn add_component_with_mode(
        &mut self,
        category: ComponentCategory,
        mode: OperationalMode,
    ) -> ComponentHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.components.push(TestComponent(id, category, mode));

        ComponentHandle(id)
    }

    /// Pops the last component added to the graph.
    pub(super) fn pop_component(&mut self) -> Option<TestComponent> {
        self.components.pop()
    }

    /// Pops the last connection added to the graph.
    pub(super) fn pop_connection(&mut self) -> Option<TestConnection> {
        self.connections.pop()
    }

    /// Adds a grid component to the graph and returns its handle.
    pub(super) fn grid(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::GridConnectionPoint)
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
        self.add_component(ComponentCategory::Inverter(InverterType::Pv))
    }

    /// Adds an EV charger to the graph and returns its handle.
    pub(super) fn ev_charger(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::EvCharger(EvChargerType::Ac))
    }

    /// Adds a CHP to the graph and returns its handle.
    pub(super) fn chp(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::Chp)
    }

    /// Adds a wind_turbine to the graph and returns its handle.
    pub(super) fn wind_turbine(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::WindTurbine)
    }

    /// Adds a steam boiler to the graph and returns its handle.
    pub(super) fn steam_boiler(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::SteamBoiler)
    }

    /// Adds a power transformer (pass-through category) to the graph.
    pub(super) fn power_transformer(&mut self) -> ComponentHandle {
        self.add_component(ComponentCategory::PowerTransformer)
    }

    /// Connects two components in the graph.
    pub(super) fn connect(&mut self, from: ComponentHandle, to: ComponentHandle) -> &mut Self {
        self.connections
            .push(TestConnection::new(from.component_id(), to.component_id()));
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
            self.connect(inverter, battery);
            batteries.push(battery);
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

    /// Adds a meter, followed by the given number of wind turbines, and
    /// returns a handle to the meter.
    pub(super) fn meter_wind_turbine_chain(&mut self, num_wind_turbines: usize) -> ComponentHandle {
        let meter = self.meter();
        for _ in 0..num_wind_turbines {
            let wind_turbine = self.wind_turbine();
            self.connect(meter, wind_turbine);
        }
        meter
    }

    /// Adds a meter, followed by the given number of steam boilers, and
    /// returns a handle to the meter.
    pub(super) fn meter_steam_boiler_chain(&mut self, num_steam_boilers: usize) -> ComponentHandle {
        let meter = self.meter();
        for _ in 0..num_steam_boilers {
            let steam_boiler = self.steam_boiler();
            self.connect(meter, steam_boiler);
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

#[cfg(test)]
mod tests {
    use super::{TestComponent, TestConnection};
    use crate::{
        BatteryType, ComponentCategory, Error, EvChargerType, InverterType, Node, OperationalMode,
    };

    fn conns(pairs: &[(u64, u64)]) -> Vec<TestConnection> {
        pairs
            .iter()
            .map(|&(s, d)| TestConnection::new(s, d))
            .collect()
    }

    #[test]
    fn test_tree_and_references() {
        let g = graph! {
            0: grid => {
                1: meter => {
                    2: inv_bat => { 4: bat, 5: bat },
                    3: inv_bat => { 4, 5 },
                },
            },
        };
        let bat = ComponentCategory::Battery(BatteryType::LiIon);
        let inv_bat = ComponentCategory::Inverter(InverterType::Battery);
        assert_eq!(
            g.components(),
            vec![
                TestComponent::new(0, ComponentCategory::GridConnectionPoint),
                TestComponent::new(1, ComponentCategory::Meter),
                TestComponent::new(2, inv_bat),
                TestComponent::new(4, bat),
                TestComponent::new(5, bat),
                TestComponent::new(3, inv_bat),
            ]
        );
        assert_eq!(
            g.connections(),
            conns(&[(0, 1), (1, 2), (2, 4), (2, 5), (1, 3), (3, 4), (3, 5)])
        );
    }

    #[test]
    fn test_kinds() {
        let g = graph! {
            0: grid, 1: meter, 2: bat, 3: bat(NaIon), 4: inv_bat, 5: inv_pv,
            6: inv_hybrid, 7: inv(Unspecified), 8: ev, 9: ev(Dc), 10: chp,
            11: wind, 12: boiler, 13: transformer, 14: unspecified,
        };
        let categories: Vec<_> = g.components().iter().map(Node::category).collect();
        assert_eq!(
            categories,
            vec![
                ComponentCategory::GridConnectionPoint,
                ComponentCategory::Meter,
                ComponentCategory::Battery(BatteryType::LiIon),
                ComponentCategory::Battery(BatteryType::NaIon),
                ComponentCategory::Inverter(InverterType::Battery),
                ComponentCategory::Inverter(InverterType::Pv),
                ComponentCategory::Inverter(InverterType::Hybrid),
                ComponentCategory::Inverter(InverterType::Unspecified),
                ComponentCategory::EvCharger(EvChargerType::Ac),
                ComponentCategory::EvCharger(EvChargerType::Dc),
                ComponentCategory::Chp,
                ComponentCategory::WindTurbine,
                ComponentCategory::SteamBoiler,
                ComponentCategory::PowerTransformer,
                ComponentCategory::Unspecified,
            ]
        );
        assert!(g.connections().is_empty());
    }

    #[test]
    fn test_modes() {
        let g = graph! {
            0: grid => {
                1: meter [Inactive] => { 2: inv_pv [ControlOnly] },
                3: bat(NaIon) [TelemetryOnly],
                4: meter,
            },
        };
        let modes: Vec<_> = g.components().iter().map(Node::operational_mode).collect();
        assert_eq!(
            modes,
            vec![
                OperationalMode::Unspecified,
                OperationalMode::Inactive,
                OperationalMode::ControlOnly,
                OperationalMode::TelemetryOnly,
                OperationalMode::Unspecified,
            ]
        );
    }

    #[test]
    fn test_forest_and_edge_only_references() {
        let g = graph! {
            7: grid => { 7 },
            0: meter => { 9 },
            1: meter,
            1 => { 0 },
        };
        let ids: Vec<_> = g.components().iter().map(Node::component_id).collect();
        assert_eq!(ids, vec![7, 0, 1]);
        assert_eq!(g.connections(), conns(&[(7, 7), (0, 9), (1, 0)]));
    }

    #[test]
    fn test_trailing_commas_and_empty_children() {
        let g = graph! {};
        assert!(g.components().is_empty() && g.connections().is_empty());

        let g = graph! { 0: grid => {}, };
        assert_eq!(g.components().len(), 1);
        assert!(g.connections().is_empty());

        let g = graph! { 0: grid => { 1: meter, }, };
        assert_eq!(g.connections(), conns(&[(0, 1)]));
    }

    #[test]
    fn test_build_sorts_components_by_id() -> Result<(), Error> {
        let g = graph! {
            0: grid => { 5: meter => { 2: meter, 1: meter } },
        };
        let declared: Vec<_> = g.components().iter().map(Node::component_id).collect();
        assert_eq!(declared, vec![0, 5, 2, 1]);

        let graph = g.build(None)?;
        let inserted: Vec<_> = graph.components().map(Node::component_id).collect();
        assert_eq!(inserted, vec![0, 1, 2, 5]);
        Ok(())
    }

    #[test]
    fn test_duplicate_declaration_is_an_error() {
        let g = graph! {
            0: grid => {
                1: inv_bat => { 2: bat },
                3: inv_bat => { 2: bat },
            },
        };
        assert!(
            g.build(None)
                .is_err_and(|e| e == Error::invalid_graph("Duplicate component ID found: 2"))
        );
    }

    #[test]
    fn test_dangling_reference_is_an_error() {
        let g = graph! { 0: grid => { 1: meter => { 9 } } };
        assert!(g.build(None).is_err_and(|e| e
            == Error::invalid_connection("Connection:(1, 9) Can't find a component with ID 9")));
    }
}
