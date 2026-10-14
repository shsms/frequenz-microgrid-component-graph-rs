// License: MIT
// Copyright © 2024 Frequenz Energy-as-a-Service GmbH

//! Methods for validating the acyclicity and connectedness of a
//! [`ComponentGraph`].

use std::collections::BTreeSet;

use crate::{Edge, Error, Node, ValidationError};

use super::ComponentGraphValidator;

impl<N, E> ComponentGraphValidator<'_, N, E>
where
    N: Node,
    E: Edge,
{
    /// Validates that all non-pass-through components are connected
    /// into a single graph.
    ///
    /// It does so by ensuring that all the components are reachable
    /// by traversing the graph from the root node. Pass-through
    /// components are exempt: they're transparent to the effective
    /// successors view, so the walk never visits them, and we don't
    /// require them to be reachable -- even when
    /// `allow_unconnected_components` is `false`.
    pub(super) fn validate_connected_graph(&self, root: &N) -> Result<(), Error> {
        let root_id = root.component_id();
        let mut visited = BTreeSet::new();
        let mut queue = vec![root_id];
        visited.insert(root_id);
        while let Some(node_id) = queue.pop() {
            for successor in self.cg.successors(node_id)? {
                visited.insert(successor.component_id());
                queue.push(successor.component_id());
            }
        }

        let unvisited = self
            .cg
            .components()
            .filter(|n| !n.category().is_passthrough())
            .map(|n| n.component_id())
            .filter(|id| !visited.contains(id))
            .collect::<Vec<_>>();

        if !unvisited.is_empty() {
            return Err(ValidationError::new(
                format!("Nodes {unvisited:?} are not connected to the root."),
                unvisited,
            )
            .into());
        }

        Ok(())
    }

    /// Validates that there are no cycles in the graph.
    ///
    /// If a cycle is detected, an error is returned, that lists the nodes in
    /// the cycle. Walks via `raw_successors` so cycles composed entirely of
    /// pass-through nodes (which are invisible to the effective view) are
    /// still detected.
    pub(super) fn validate_acyclicity(
        &self,
        node: &N,
        mut predecessors: Vec<u64>,
    ) -> Result<(), Error> {
        predecessors.push(node.component_id());
        for successor in self.cg.raw_successors(node.component_id())? {
            if let Some(first_occurrence) = predecessors
                .iter()
                .position(|id| *id == successor.component_id())
            {
                let cycle = &predecessors[first_occurrence..];
                return Err(ValidationError::new(
                    format!(
                        "Cycle detected: {} -> {}",
                        cycle
                            .iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(" -> "),
                        successor.component_id()
                    ),
                    cycle.to_vec(),
                )
                .into());
            }
            self.validate_acyclicity(successor, predecessors.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::ComponentGraph;
    use crate::ComponentGraphConfig;
    use crate::error::Error;
    use crate::graph::test_utils::{
        TestComponent, TestConnection, TestGraph, graph, validation_error,
    };

    /// Builds `g` with its components in declaration order; `TestGraph::build`
    /// would sort them by ID.
    ///
    /// `test_connected_graph_validation` and `test_acyclicity_validation`
    /// declare components and connections out of ID order, to check that
    /// validation still works when the input is not sorted.
    fn build_in_declaration_order(
        g: &TestGraph,
        config: Option<ComponentGraphConfig>,
    ) -> Result<ComponentGraph<TestComponent, TestConnection>, Error> {
        ComponentGraph::try_new(
            g.components().to_vec(),
            g.connections().to_vec(),
            config.unwrap_or_default(),
        )
    }

    #[test]
    fn test_connected_graph_validation() {
        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
        };
        assert!(build_in_declaration_order(&g, None).is_ok());

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            11: meter,
        };
        let Err(err) = build_in_declaration_order(&g, None) else {
            panic!()
        };
        assert!(
            build_in_declaration_order(&g, None).is_err_and(
                |e| e == validation_error("Nodes [11] are not connected to the root.", &[11])
            ),
            "{err:?}"
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            11: meter, 12: meter,
        };
        assert!(
            build_in_declaration_order(&g, None).is_err_and(|e| e
                == validation_error("Nodes [11, 12] are not connected to the root.", &[11, 12]))
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            11: meter => { 12: meter },
        };
        // With the default config, this fails validation.
        assert!(
            build_in_declaration_order(&g, None).is_err_and(|e| e
                == validation_error("Nodes [11, 12] are not connected to the root.", &[11, 12]))
        );
        // With `allow_unconnected_components=true`, this passes validation.
        assert!(
            build_in_declaration_order(
                &g,
                Some(
                    ComponentGraphConfig::builder()
                        .allow_unconnected_components(true)
                        .build(),
                ),
            )
            .is_ok()
        );
    }

    #[test]
    fn test_acyclicity_validation() {
        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
        };
        assert!(build_in_declaration_order(&g, None).is_ok());

        // add cycles at different levels
        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            3 => { 2 },
        };
        assert!(
            build_in_declaration_order(&g, None)
                .is_err_and(|e| e == validation_error("Cycle detected: 2 -> 3 -> 2", &[2, 3])),
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            4 => { 2 },
        };
        assert!(
            build_in_declaration_order(&g, None).is_err_and(
                |e| e == validation_error("Cycle detected: 2 -> 3 -> 4 -> 2", &[2, 3, 4])
            )
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            5 => { 2 },
        };
        assert!(build_in_declaration_order(&g, None).is_err_and(
            |e| e == validation_error("Cycle detected: 2 -> 3 -> 4 -> 5 -> 2", &[2, 3, 4, 5])
        ));

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            4 => { 3 },
        };
        assert!(
            build_in_declaration_order(&g, None)
                .is_err_and(|e| e == validation_error("Cycle detected: 3 -> 4 -> 3", &[3, 4]))
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            5 => { 3 },
        };
        assert!(
            build_in_declaration_order(&g, None).is_err_and(
                |e| e == validation_error("Cycle detected: 3 -> 4 -> 5 -> 3", &[3, 4, 5])
            )
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            5 => { 4 },
        };
        assert!(
            build_in_declaration_order(&g, None)
                .is_err_and(|e| e == validation_error("Cycle detected: 4 -> 5 -> 4", &[4, 5]))
        );

        let g = graph! {
            3: meter => { 4: inv_bat => { 5: bat(Unspecified) } },
            6: meter => { 7: inv_bat => { 8: bat(Unspecified) } },
            9: meter => { 10: inv_pv },
            1: grid => { 2: meter => { 3, 6, 9 } },
            9 => { 2 },
        };
        assert!(
            build_in_declaration_order(&g, None)
                .is_err_and(|e| e == validation_error("Cycle detected: 2 -> 9 -> 2", &[2, 9]))
        );
    }

    /// A tolerated component-rule violation must not cancel out an
    /// unconnected-component failure that is *not* tolerated: the checks'
    /// outcomes are combined, not overwritten by whichever runs last.
    #[test]
    fn test_tolerated_failure_does_not_mask_a_fatal_one() {
        let g = graph! {
            // The meter -> battery edge breaks a neighbor rule, which *is*
            // tolerated.
            1: grid => { 2: meter => { 3: bat } },
            // Node 4 is left unconnected, which is not tolerated by default.
            4: meter,
        };
        let config = ComponentGraphConfig::builder()
            .allow_component_validation_failures(true)
            .build();

        assert!(g.build(Some(config)).is_err());
    }

    /// A pass-through-only cycle attached to an otherwise-valid graph
    /// is rejected at construction time. The acyclicity validator
    /// walks the raw graph so cycles composed entirely of pass-through
    /// nodes are still detected.
    #[test]
    fn test_acyclicity_detects_passthrough_only_cycle() {
        let g = graph! {
            0: grid => {
                1: meter => { 2: inv_bat => { 3: bat } },
                4: transformer => { 5: transformer => { 6: transformer => { 4 } } },
            },
        };

        assert!(
            g.build(None).is_err(),
            "PT-only cycle reachable from the GCP must be detected at construction time"
        );
    }
}
