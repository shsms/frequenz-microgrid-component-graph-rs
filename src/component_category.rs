// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module defines the `ComponentCategory` enum, which represents the
//! category of a component.

use crate::graph_traits::Node;
use crate::ComponentGraphConfig;
use std::fmt::Display;

/// Represents the type of an inverter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InverterType {
    Unspecified,
    Solar,
    Battery,
    Hybrid,
}

impl Display for InverterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InverterType::Unspecified => write!(f, "Unspecified"),
            InverterType::Solar => write!(f, "Solar"),
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
    Grid,
    Meter,
    Battery(BatteryType),
    Inverter(InverterType),
    EvCharger(EvChargerType),
    Converter,
    CryptoMiner,
    Electrolyzer,
    Chp,
    Precharger,
    Fuse,
    VoltageTransformer,
    Hvac,
    Relay,
}

impl Display for ComponentCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentCategory::Unspecified => write!(f, "Unspecified"),
            ComponentCategory::Grid => write!(f, "Grid"),
            ComponentCategory::Meter => write!(f, "Meter"),
            ComponentCategory::Battery(battery_type) => write!(f, "Battery({})", battery_type),
            ComponentCategory::Inverter(inverter_type) => write!(f, "{}Inverter", inverter_type),
            ComponentCategory::EvCharger(ev_charger_type) => {
                write!(f, "EVCharger({})", ev_charger_type)
            }
            ComponentCategory::Converter => write!(f, "Converter"),
            ComponentCategory::CryptoMiner => write!(f, "CryptoMiner"),
            ComponentCategory::Electrolyzer => write!(f, "Electrolyzer"),
            ComponentCategory::Chp => write!(f, "CHP"),
            ComponentCategory::Precharger => write!(f, "Precharger"),
            ComponentCategory::Fuse => write!(f, "Fuse"),
            ComponentCategory::VoltageTransformer => write!(f, "VoltageTransformer"),
            ComponentCategory::Hvac => write!(f, "HVAC"),
            ComponentCategory::Relay => write!(f, "Relay"),
        }
    }
}

/// Predicates for checking the component category of a `Node`.
pub(crate) trait CategoryPredicates: Node {
    fn is_unspecified(&self) -> bool {
        self.category() == ComponentCategory::Unspecified
    }

    fn is_grid(&self) -> bool {
        self.category() == ComponentCategory::Grid
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
        self.category() == ComponentCategory::Inverter(InverterType::Solar)
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
}

/// Implement the `CategoryPredicates` trait for all types that implement the
/// `Node` trait.
impl<T: Node> CategoryPredicates for T {}
