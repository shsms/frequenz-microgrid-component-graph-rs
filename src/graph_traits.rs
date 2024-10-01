// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! This module contains the traits that need to be implemented by the types
//! that represent a node and an edge.

use crate::component_category::ComponentCategory;

/**
This trait needs to be implemented by the type that represents a node.

Read more about why this is necessary [here][crate#the-node-and-edge-traits].

<details>
<summary>Example implementation for microgrid API v0.17:</summary>

```ignore
impl component_graph::Node for common::v1::microgrid::components::Component {
    fn component_id(&self) -> u64 {
        self.id as u64
    }

    fn category(&self) -> component_graph::ComponentCategory {
        use common::v1::microgrid::components as pb;
        use component_graph as gr;

        let category = pb::ComponentCategory::try_from(self.category).unwrap_or_else(|e| {
        error!("Error converting component category: {}. Component ID: {}", e, self.id);
            pb::ComponentCategory::Unspecified
        });

        match category {
            pb::ComponentCategory::Unspecified => gr::ComponentCategory::Unspecified,
            pb::ComponentCategory::Grid => gr::ComponentCategory::Grid,
            pb::ComponentCategory::Meter => gr::ComponentCategory::Meter,
            pb::ComponentCategory::Inverter => {
                gr::ComponentCategory::Inverter(match self.category_type {
                    Some(pb::ComponentCategoryMetadataVariant { metadata }) => match metadata {
                        Some(pb::component_category_metadata_variant::Metadata::Inverter(
                            inverter,
                        )) => match pb::InverterType::try_from(inverter.r#type).unwrap() {
                            pb::InverterType::Solar => gr::InverterType::Solar,
                            pb::InverterType::Battery => gr::InverterType::Battery,
                            pb::InverterType::Hybrid => gr::InverterType::Hybrid,
                            pb::InverterType::Unspecified => gr::InverterType::Unspecified,
                        },
                        Some(_) => {
                            warn!("Unknown metadata variant for inverter: {:?}", metadata);
                            gr::InverterType::Unspecified
                        }
                        None => gr::InverterType::Unspecified,
                    },
                    _ => gr::InverterType::Unspecified,
                })
            }
            pb::ComponentCategory::Converter => gr::ComponentCategory::Converter,
            pb::ComponentCategory::Battery => gr::ComponentCategory::Battery,
            pb::ComponentCategory::EvCharger => gr::ComponentCategory::EvCharger,
            pb::ComponentCategory::CryptoMiner => gr::ComponentCategory::CryptoMiner,
            pb::ComponentCategory::Electrolyzer => gr::ComponentCategory::Electrolyzer,
            pb::ComponentCategory::Chp => gr::ComponentCategory::Chp,
            pb::ComponentCategory::Relay => gr::ComponentCategory::Relay,
            pb::ComponentCategory::Precharger => gr::ComponentCategory::Precharger,
            pb::ComponentCategory::Fuse => gr::ComponentCategory::Fuse,
            pb::ComponentCategory::VoltageTransformer => gr::ComponentCategory::VoltageTransformer,
            pb::ComponentCategory::Hvac => gr::ComponentCategory::Hvac,
        }
    }

    fn is_supported(&self) -> bool {
        self.status != common::v1::microgrid::components::ComponentStatus::Inactive as i32
    }
}
```

</details>
*/
pub trait Node {
    /// Returns the component id of the component.
    fn component_id(&self) -> u64;
    /// Returns the category of the category.
    fn category(&self) -> ComponentCategory;
    /// Returns true if the component can be read from and/or controlled.
    fn is_supported(&self) -> bool;
}

/**
This trait needs to be implemented by the type that represents a connection.

Read more about why this is necessary [here][crate#the-node-and-edge-traits].

<details>
<summary>Example implementation for microgrid API v0.17:</summary>

```ignore
impl component_graph::Edge for common::v1::microgrid::components::ComponentConnection {
    fn source(&self) -> u64 {
        self.source_component_id
    }

    fn destination(&self) -> u64 {
        self.destination_component_id
    }
}
```

</details>
*/
pub trait Edge {
    /// Returns the source component id of the connection.
    fn source(&self) -> u64;
    /// Returns the destination component id of the connection.
    fn destination(&self) -> u64;
}
