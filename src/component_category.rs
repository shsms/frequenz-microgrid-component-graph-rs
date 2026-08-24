// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the `ComponentCategory` enum, which represents the
//! category of a component.

use crate::ComponentGraphConfig;
use crate::graph_traits::Node;
use std::fmt::Display;

/// Represents the type of an inverter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InverterType {
    Unspecified,
    Pv,
    Battery,
    Hybrid,
}

impl Display for InverterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InverterType::Unspecified => write!(f, "Unspecified"),
            InverterType::Pv => write!(f, "Pv"),
            InverterType::Battery => write!(f, "Battery"),
            InverterType::Hybrid => write!(f, "Hybrid"),
        }
    }
}

/// Represents the type of a battery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BatteryType {
    Unspecified,
    LiIon,
    NaIon,
}

impl Display for BatteryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatteryType::Unspecified => write!(f, "Unspecified"),
            BatteryType::LiIon => write!(f, "LiIon"),
            BatteryType::NaIon => write!(f, "NaIon"),
        }
    }
}

/// Represents the type of an EV charger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EvChargerType {
    Unspecified,
    Ac,
    Dc,
    Hybrid,
}

impl Display for EvChargerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvChargerType::Unspecified => write!(f, "Unspecified"),
            EvChargerType::Ac => write!(f, "AC"),
            EvChargerType::Dc => write!(f, "DC"),
            EvChargerType::Hybrid => write!(f, "Hybrid"),
        }
    }
}

/// Represents the category of a component.
///
/// Values of the underlying generated `ComponentCategory` and `ComponentType` types
/// need to be converted to this type, so that they can be used in the
/// `ComponentGraph`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ComponentCategory {
    Unspecified,
    GridConnectionPoint,
    Meter,
    Inverter(InverterType),
    Converter,
    Battery(BatteryType),
    EvCharger(EvChargerType),
    Breaker,
    Precharger,
    Chp,
    Electrolyzer,
    PowerTransformer,
    Hvac,
    Plc,
    CryptoMiner,
    StaticTransferSwitch,
    UninterruptiblePowerSupply,
    CapacitorBank,
    WindTurbine,
    SteamBoiler,
}

impl Display for ComponentCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentCategory::Unspecified => write!(f, "Unspecified"),
            ComponentCategory::GridConnectionPoint => write!(f, "GridConnectionPoint"),
            ComponentCategory::Meter => write!(f, "Meter"),
            ComponentCategory::Battery(battery_type) => write!(f, "Battery({battery_type})"),
            ComponentCategory::Inverter(inverter_type) => write!(f, "{inverter_type}Inverter"),
            ComponentCategory::EvCharger(ev_charger_type) => {
                write!(f, "EVCharger({ev_charger_type})")
            }
            ComponentCategory::Converter => write!(f, "Converter"),
            ComponentCategory::CryptoMiner => write!(f, "CryptoMiner"),
            ComponentCategory::Electrolyzer => write!(f, "Electrolyzer"),
            ComponentCategory::Chp => write!(f, "CHP"),
            ComponentCategory::Precharger => write!(f, "Precharger"),
            ComponentCategory::Hvac => write!(f, "HVAC"),
            ComponentCategory::Breaker => write!(f, "Breaker"),
            ComponentCategory::PowerTransformer => write!(f, "PowerTransformer"),
            ComponentCategory::Plc => write!(f, "PLC"),
            ComponentCategory::StaticTransferSwitch => write!(f, "StaticTransferSwitch"),
            ComponentCategory::UninterruptiblePowerSupply => {
                write!(f, "UninterruptiblePowerSupply")
            }
            ComponentCategory::CapacitorBank => write!(f, "CapacitorBank"),
            ComponentCategory::WindTurbine => write!(f, "WindTurbine"),
            ComponentCategory::SteamBoiler => write!(f, "SteamBoiler"),
        }
    }
}

impl ComponentCategory {
    /// A human-readable label for explanation prose, e.g. "PV inverter".
    ///
    /// Lower-case except for acronyms; capitalize the first letter when it
    /// starts a sentence.
    pub(crate) fn label(self) -> &'static str {
        use ComponentCategory as C;
        match self {
            C::Unspecified => "component",
            C::GridConnectionPoint => "grid connection point",
            C::Meter => "meter",
            C::Inverter(InverterType::Pv) => "PV inverter",
            C::Inverter(InverterType::Battery) => "battery inverter",
            C::Inverter(InverterType::Hybrid) => "hybrid inverter",
            C::Inverter(InverterType::Unspecified) => "inverter",
            C::Battery(_) => "battery",
            C::EvCharger(_) => "EV charger",
            C::Chp => "CHP",
            C::WindTurbine => "wind turbine",
            C::SteamBoiler => "steam boiler",
            C::Converter => "converter",
            C::Breaker => "breaker",
            C::Precharger => "precharger",
            C::Electrolyzer => "electrolyzer",
            C::PowerTransformer => "power transformer",
            C::Hvac => "HVAC unit",
            C::Plc => "PLC",
            C::CryptoMiner => "crypto miner",
            C::StaticTransferSwitch => "static transfer switch",
            C::UninterruptiblePowerSupply => "UPS",
            C::CapacitorBank => "capacitor bank",
        }
    }

    /// Returns `true` if this category is a *pass-through*: a component
    /// that has no specific handling in the graph and should be treated
    /// as transparent by validators and formula generators.
    ///
    /// Pass-through nodes can sit anywhere in the chain between handled
    /// categories. They don't generate or consume their own measurement,
    /// and neighbor relationships across them are evaluated by walking
    /// past them as if they weren't present.
    ///
    /// `Unspecified` is intentionally not a pass-through: it's rejected
    /// at graph-construction time before validators or formula
    /// generators ever see it.
    pub(crate) fn is_passthrough(self) -> bool {
        use ComponentCategory as C;
        matches!(
            self,
            C::Converter
                | C::Breaker
                | C::Precharger
                | C::Electrolyzer
                | C::PowerTransformer
                | C::Hvac
                | C::Plc
                | C::CryptoMiner
                | C::StaticTransferSwitch
                | C::UninterruptiblePowerSupply
                | C::CapacitorBank
        )
    }
}

/// Predicates for checking the component category of a `Node`.
pub(crate) trait CategoryPredicates: Node {
    fn is_unspecified(&self) -> bool {
        self.category() == ComponentCategory::Unspecified
    }

    fn is_grid(&self) -> bool {
        self.category() == ComponentCategory::GridConnectionPoint
    }

    fn is_meter(&self) -> bool {
        self.category() == ComponentCategory::Meter
    }

    fn is_battery(&self) -> bool {
        matches!(self.category(), ComponentCategory::Battery(_))
    }

    fn is_inverter(&self) -> bool {
        matches!(self.category(), ComponentCategory::Inverter(_))
    }

    fn is_battery_inverter(&self, config: &ComponentGraphConfig) -> bool {
        match self.category() {
            ComponentCategory::Inverter(InverterType::Battery) => true,
            ComponentCategory::Inverter(InverterType::Unspecified) => {
                config.allow_unspecified_inverters
            }
            _ => false,
        }
    }

    fn is_pv_inverter(&self) -> bool {
        self.category() == ComponentCategory::Inverter(InverterType::Pv)
    }

    fn is_hybrid_inverter(&self) -> bool {
        self.category() == ComponentCategory::Inverter(InverterType::Hybrid)
    }

    fn is_unspecified_inverter(&self, config: &ComponentGraphConfig) -> bool {
        match self.category() {
            ComponentCategory::Inverter(InverterType::Unspecified) => {
                !config.allow_unspecified_inverters
            }
            _ => false,
        }
    }

    fn is_ev_charger(&self) -> bool {
        matches!(self.category(), ComponentCategory::EvCharger(_))
    }

    fn is_chp(&self) -> bool {
        self.category() == ComponentCategory::Chp
    }

    fn is_wind_turbine(&self) -> bool {
        self.category() == ComponentCategory::WindTurbine
    }

    fn is_steam_boiler(&self) -> bool {
        self.category() == ComponentCategory::SteamBoiler
    }

    /// Returns `true` if the component provides telemetry data, and so can be a
    /// measurement source in a formula.
    ///
    /// Determined by the component's [operational mode][Node::operational_mode].
    /// A component that does not provide telemetry is still used to classify the
    /// meter that measures it (e.g. as a PV meter or a CHP meter).
    fn provides_telemetry(&self) -> bool {
        self.operational_mode().provides_telemetry()
    }
}

/// Implement the `CategoryPredicates` trait for all types that implement the
/// `Node` trait.
impl<T: Node> CategoryPredicates for T {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks the pass-through classification. If a category is added or
    /// re-classified, this test forces an explicit decision rather than
    /// silently inheriting the default.
    #[test]
    fn test_is_passthrough_classification() {
        // Pass-through categories: no specific validation rule, no formula
        // contribution; transparent to traversal.
        for c in [
            ComponentCategory::Converter,
            ComponentCategory::Breaker,
            ComponentCategory::Precharger,
            ComponentCategory::Electrolyzer,
            ComponentCategory::PowerTransformer,
            ComponentCategory::Hvac,
            ComponentCategory::Plc,
            ComponentCategory::CryptoMiner,
            ComponentCategory::StaticTransferSwitch,
            ComponentCategory::UninterruptiblePowerSupply,
            ComponentCategory::CapacitorBank,
        ] {
            assert!(c.is_passthrough(), "{c} should be a pass-through");
        }

        // Handled or special-cased categories.
        for c in [
            ComponentCategory::Unspecified,
            ComponentCategory::GridConnectionPoint,
            ComponentCategory::Meter,
            ComponentCategory::Inverter(InverterType::Battery),
            ComponentCategory::Inverter(InverterType::Pv),
            ComponentCategory::Inverter(InverterType::Hybrid),
            ComponentCategory::Inverter(InverterType::Unspecified),
            ComponentCategory::Battery(BatteryType::LiIon),
            ComponentCategory::Battery(BatteryType::NaIon),
            ComponentCategory::Battery(BatteryType::Unspecified),
            ComponentCategory::EvCharger(EvChargerType::Ac),
            ComponentCategory::EvCharger(EvChargerType::Dc),
            ComponentCategory::EvCharger(EvChargerType::Hybrid),
            ComponentCategory::EvCharger(EvChargerType::Unspecified),
            ComponentCategory::Chp,
            ComponentCategory::WindTurbine,
        ] {
            assert!(!c.is_passthrough(), "{c} should not be a pass-through");
        }
    }
}
