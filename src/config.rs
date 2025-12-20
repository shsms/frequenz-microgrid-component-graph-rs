// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the configuration options for the `ComponentGraph`.

/// Configuration options for the `ComponentGraph`.
#[derive(Clone, Default, Debug)]
pub struct ComponentGraphConfig {
    /// Whether to allow validation errors on components.  When this is `true`,
    /// the graph will be built even if there are validation errors on
    /// components.
    pub allow_component_validation_failures: bool,

    /// Whether to allow unconnected components in the graph, that are not
    /// reachable from the root.
    pub allow_unconnected_components: bool,

    /// Whether to allow untyped inverters in the graph.  When this is `true`,
    /// inverters that have `InverterType::Unspecified` will be assumed to be
    /// Battery inverters.
    pub allow_unspecified_inverters: bool,

    /// Whether to disable fallback components in generated formulas.  When this
    /// is `true`, the formulas will not include fallback components.
    pub disable_fallback_components: bool,

    /// Whether to prefer PV inverters when generating PV formulas.  When this
    /// is `true`, PV inverters will be the primary source and PV meters will be
    /// the fallback.  When `false`, PV meters will be the primary source.
    pub prefer_inverters_in_pv_formula: bool,

    /// Whether to prefer battery inverters when generating Battery formulas.
    /// When this is `true`, battery inverters will be the primary source and
    /// battery meters will be secondary.  When `false`, battery meters will be
    /// the primary source.
    pub prefer_inverters_in_battery_formula: bool,

    /// Whether to prefer CHP when generating CHP formulas.  When this is
    /// `true`, CHP units will be the primary source and CHP meters will be
    /// secondary.  When `false`, CHP meters will be the primary source.
    pub prefer_chp_in_chp_formula: bool,
    /// Whether to prefer wind turbines when generating Wind formulas.  When
    /// this is `true`, wind turbines will be the primary source and wind meters
    /// will be secondary.  When `false`, wind meters will be the primary
    /// source.
    pub prefer_wind_turbines_in_wind_formula: bool,
}
