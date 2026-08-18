use accesskit::{Node, NodeId, Role};
use dioxus_native_dom::DioxusDocument;
use std::collections::HashMap;

/// A representation of the ARIA tree constructed from a document.
///
/// This provides an efficient way to look up accesskit [`Node`]s as well as an algorithm to compute
/// the accessible name of a given node.
pub(super) struct AriaTree {
    nodes_by_node_id: HashMap<NodeId, Node>,
}

impl AriaTree {
    /// Constructs a new [`AriaTree`] from the data in the DOM of the given [`DioxusDocument`].
    pub(super) fn for_document(document: &DioxusDocument) -> Self {
        let tree = document.inner.borrow().build_accessibility_tree();
        let nodes_by_node_id = tree.nodes.into_iter().collect();
        Self { nodes_by_node_id }
    }

    /// Returns the accesskit [Node] with the given [NodeId], if it exists.
    pub(super) fn get_node(&self, node_id: NodeId) -> Option<&Node> {
        self.nodes_by_node_id.get(&node_id)
    }

    /// Computes the W3C accessible name of the given accesskit [`Node`].
    ///
    /// The algorithm implements a subset of the
    /// [specification](https://w3c.github.io/aria/accname/#computation-steps). Currently this only
    /// supports constructing the name via text content from descendents of the node. Separate
    /// labels, tooltips, and ARIA label attributes are not supported.
    // TODO: Support more of the above spec once the prerequisites in Blitz are in place.
    //
    // As of the time of writing, Blitz does not appear to set the label on accesskit nodes when
    // building the accessibility tree. Nor does it appear to support <label> elements at all. For
    // example, there is no code which evaluates the `for`  attribute of a `<label>` element and
    // sets the target element's `labelled_by` accordingly. Without these features, there is no
    // meaningful way to test the corresponding implementation in this crate.
    pub(super) fn compute_accessible_name(&self, node: &Node) -> String {
        self.compute_accessible_name_recursively(node, false)
            .unwrap_or_default()
    }

    fn compute_accessible_name_recursively(
        &self,
        node: &Node,
        always_allow_name_from_content: bool,
    ) -> Option<String> {
        if (always_allow_name_from_content || Self::supports_name_from_content(node.role()))
            && let Some(text_content) = self.get_text_content(node)
        {
            Some(text_content)
        } else if matches!(node.role(), Role::TextRun)
            && let Some(value) = node.value()
        {
            Some(value.to_string())
        } else {
            None
        }
    }

    // From https://w3c.github.io/aria/#namefromcontent
    fn supports_name_from_content(role: Role) -> bool {
        matches!(
            role,
            Role::Button
                | Role::Cell
                | Role::CheckBox
                | Role::ColumnHeader
                | Role::Comment
                | Role::GridCell
                | Role::Heading
                | Role::Link
                | Role::MenuItem
                | Role::MenuItemCheckBox
                | Role::MenuItemRadio
                | Role::ListBoxOption
                | Role::RadioButton
                | Role::Row
                | Role::RowHeader
                | Role::Switch
                | Role::Tab
                | Role::TreeItem
        )
    }

    fn get_text_content(&self, node: &Node) -> Option<String> {
        let parts: Vec<_> = node
            .children()
            .iter()
            .filter_map(|child_id| self.nodes_by_node_id.get(child_id))
            .filter_map(|child| self.compute_accessible_name_recursively(child, true))
            .collect();
        if !parts.is_empty() {
            Some(parts.join(" "))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    mod compute_accessible_name {
        use crate::{
            DocumentTester, ResolvedElement, by_role, by_testid, query::aria_tree::AriaTree, render,
        };
        use accesskit::Role;
        use blitz_dom::Document as _;
        use dioxus::prelude::*;
        use std::ops::Deref;
        use test_that::prelude::*;

        #[test]
        fn obtains_name_from_text_node_inside_button() -> TestResult<()> {
            #[component]
            fn TestComponent() -> Element {
                rsx! {
                    button {
                        "Text node content"
                    }
                }
            }
            let tester = render(TestComponent);
            let aria_tree = build_aria_tree(&tester);
            let starting_node = get_accesskit_node_of_element(
                &aria_tree,
                &tester.query(by_role(Role::Button)).immediately()?,
            );

            let result = aria_tree.compute_accessible_name(starting_node);

            verify_that!(result, eq("Text node content"))
        }

        #[test]
        fn concatenates_content_of_text_nodes() -> TestResult<()> {
            #[component]
            fn TestComponent() -> Element {
                rsx! {
                    button {
                        div { "Text node" }
                        div { "content" }
                    }
                }
            }
            let tester = render(TestComponent);
            let aria_tree = build_aria_tree(&tester);
            let starting_node = get_accesskit_node_of_element(
                &aria_tree,
                &tester.query(by_role(Role::Button)).immediately()?,
            );

            let result = aria_tree.compute_accessible_name(starting_node);

            verify_that!(result, eq("Text node content"))
        }

        #[test]
        fn returns_empty_string_if_node_has_no_accessible_name() -> TestResult<()> {
            #[component]
            fn TestComponent() -> Element {
                rsx! {
                    div {
                        "data-testid": "node",
                        "Text node content"
                    }
                }
            }
            let tester = render(TestComponent);
            let aria_tree = build_aria_tree(&tester);
            let starting_node = get_accesskit_node_of_element(
                &aria_tree,
                &tester.query(by_testid("node")).immediately()?,
            );

            let result = aria_tree.compute_accessible_name(starting_node);

            verify_that!(result, eq(""))
        }

        fn build_aria_tree(tester: &DocumentTester) -> AriaTree {
            tester.build();
            AriaTree::for_document(tester.document().deref())
        }

        fn get_accesskit_node_of_element<'nodes>(
            aria_tree: &'nodes AriaTree,
            element: &ResolvedElement,
        ) -> &'nodes accesskit::Node {
            let accesskit_node_id = accesskit::NodeId(
                element
                    .node_id
                    .into_raw_id(&element.document.borrow().inner()) as u64,
            );
            aria_tree
                .get_node(accesskit_node_id)
                .expect("Node must be present")
        }
    }
}
