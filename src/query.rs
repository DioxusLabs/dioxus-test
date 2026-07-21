use crate::TesterError;
use accesskit::{Node, Role};
use blitz_dom::{Document as _, SelectorList};
use dioxus_native_dom::DioxusDocument;
use style::dom_apis::{MayUseInvalidation, QueryFirst, query_selector};

/// A value which can be turned into a CSS selector to query the DOM.
///
/// This is implemented for all types which dereference to `str`, including `&str` and `String`.
///
/// One can also select by [testid](https://testing-library.com/docs/queries/bytestid/) using the
/// function [by_testid].
pub trait Query: std::fmt::Display {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<usize>;

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<usize>;

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError;

    fn render_parent_dom(&self, document: &DioxusDocument) -> String;
}

pub trait IntoQuery {
    type Query: CloneableQuery;

    fn into_query(self) -> Self::Query;
}

pub trait CloneableQuery: ParentableQuery + Clone {}

impl<T: ParentableQuery + Clone> CloneableQuery for T {}

pub trait ParentableQuery: Query {
    fn with_parent(self, parent: &dyn Query) -> impl CloneableQuery;
}

#[derive(Clone)]
pub struct CssSelectorQuery<'parent, T>(T, Option<&'parent dyn Query>);

impl<T: AsRef<str> + std::fmt::Display + Clone> IntoQuery for T {
    type Query = CssSelectorQuery<'static, T>;

    fn into_query(self) -> Self::Query {
        CssSelectorQuery(self, None)
    }
}

impl<'parent, T: std::fmt::Display> std::fmt::Display for CssSelectorQuery<'parent, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'parent, T: AsRef<str> + std::fmt::Display + Clone> Query for CssSelectorQuery<'parent, T> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<usize> {
        let selector_list = self
            .parse_css_selector_to_query(document)
            .expect("Error parsing CSS selector");
        let doc_guard = document.inner();
        let start_node = if let Some(parent) = self.1 {
            doc_guard.get_node(parent.get_first_element(document)?)?
        } else {
            doc_guard.root_node()
        };
        let mut result = None;
        query_selector::<&blitz_dom::Node, QueryFirst>(
            start_node,
            &selector_list,
            &mut result,
            MayUseInvalidation::Yes,
        );
        result.map(|node| node.id)
    }

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<usize> {
        document
            .inner()
            .query_selector_all_raw(&self.parse_css_selector_to_query(document).unwrap())
            .to_vec()
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        match self.1 {
            Some(c) => match c.get_first_element(document) {
                Some(element) => document
                    .inner()
                    .get_node(element)
                    .expect("Expected to find node")
                    .outer_html_pretty(),
                None => c.render_parent_dom(document),
            },
            None => document.inner().root_element().outer_html_pretty(),
        }
    }

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.1
            && parent.get_first_element(document).is_none()
        {
            parent.describe_failure(document)
        } else {
            TesterError::NoSuchElementWithCssSelector(
                self.0.as_ref().into(),
                self.render_parent_dom(document),
            )
        }
    }
}

impl<'parent, T: AsRef<str> + std::fmt::Display + Clone> CssSelectorQuery<'parent, T> {
    fn parse_css_selector_to_query(
        &self,
        document: &DioxusDocument,
    ) -> Result<SelectorList, TesterError> {
        document
            .inner()
            .try_parse_selector_list(self.0.as_ref())
            .map_err(|_| {
                TesterError::InvalidCssSelector(format!(
                    "Invalid CSS selector `{}`",
                    self.0.as_ref()
                ))
            })
    }
}

impl<'parent, T: AsRef<str> + std::fmt::Display + Clone> ParentableQuery
    for CssSelectorQuery<'parent, T>
{
    fn with_parent(self, parent: &dyn Query) -> impl CloneableQuery {
        CssSelectorQuery(self.0, Some(parent))
    }
}

#[derive(Clone)]
struct QueryByTestId<'parent>(String, Option<&'parent dyn Query>);

impl<'parent> Query for QueryByTestId<'parent> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<usize> {
        let selector_list = self.create_selector(document);
        let doc_guard = document.inner();
        let start_node = if let Some(parent) = self.1 {
            doc_guard.get_node(parent.get_first_element(document)?)?
        } else {
            doc_guard.root_node()
        };
        let mut result = None;
        query_selector::<&blitz_dom::Node, QueryFirst>(
            start_node,
            &selector_list,
            &mut result,
            MayUseInvalidation::Yes,
        );
        result.map(|node| node.id)
    }

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<usize> {
        document
            .inner()
            .query_selector_all_raw(&self.create_selector(document))
            .to_vec()
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        match self.1 {
            Some(c) => match c.get_first_element(document) {
                Some(element) => document
                    .inner()
                    .get_node(element)
                    .expect("Expected to find node")
                    .outer_html_pretty(),
                None => c.render_parent_dom(document),
            },
            None => document.inner().root_element().outer_html_pretty(),
        }
    }

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.1
            && parent.get_first_element(document).is_none()
        {
            parent.describe_failure(document)
        } else {
            TesterError::NoSuchElementWithTestId(self.0.clone(), self.render_parent_dom(document))
        }
    }
}

impl<'parent> QueryByTestId<'parent> {
    fn create_selector(&self, document: &DioxusDocument) -> SelectorList {
        document
            .inner()
            .try_parse_selector_list(&format!(r#"[data-testid="{}"]"#, self.0))
            .expect("Selector with testid should always parse")
    }
}

impl<'parent> ParentableQuery for QueryByTestId<'parent> {
    fn with_parent(self, parent: &dyn Query) -> impl CloneableQuery {
        QueryByTestId(self.0, Some(parent))
    }
}

impl<'parent> std::fmt::Display for QueryByTestId<'parent> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"[data-testid="{}"]"#, self.0)
    }
}

impl<'parent> IntoQuery for QueryByTestId<'parent> {
    type Query = Self;

    fn into_query(self) -> Self::Query {
        self
    }
}

/// Returns a query selector matching elements with the given value in the `data-testid` attribute.
///
/// ```
/// use dioxus::prelude::*;
/// use dioxus_test::{by_testid, matchers::{eq, inner_html}, render};
///
/// #[component]
/// fn MyComponent() -> Element {
///     rsx! {
///         div {
///              "data-testid": "the-label",
///              "Label content"
///         }
///     }
/// }
///
/// let tester = render(MyComponent).build();
/// tester
///     .query(by_testid("the-label"))
///     .expect(inner_html(eq("Label content")))
///     .immediately()
///     .unwrap();
/// ```
///
/// This attribute is a common convention for marking DOM components with which tests interact. Find
/// more information [here](https://testing-library.com/docs/queries/bytestid/).
pub fn by_testid(testid: impl AsRef<str>) -> impl IntoQuery {
    QueryByTestId(testid.as_ref().to_string(), None)
}

#[derive(Clone)]
struct QueryByRole<'parent>(Role, Option<&'parent dyn Query>);

impl<'parent> Query for QueryByRole<'parent> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<usize> {
        let tree = document.inner.borrow().build_accessibility_tree();
        let starting_node_id = self.get_starting_node_id(document)?;
        self.find_first_element_starting_at(accesskit::NodeId(starting_node_id as u64), &tree.nodes)
    }

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<usize> {
        let tree = document.inner.borrow().build_accessibility_tree();
        let Some(starting_node_id) = self.get_starting_node_id(document) else {
            return vec![];
        };
        self.find_all_elements_starting_at(accesskit::NodeId(starting_node_id as u64), &tree.nodes)
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        match self.1 {
            Some(c) => match c.get_first_element(document) {
                Some(element) => document
                    .inner()
                    .get_node(element)
                    .expect("Expected to find node")
                    .outer_html_pretty(),
                None => c.render_parent_dom(document),
            },
            None => document.inner().root_element().outer_html_pretty(),
        }
    }

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.1
            && parent.get_first_element(document).is_none()
        {
            parent.describe_failure(document)
        } else {
            TesterError::NoSuchElementWithRole(
                format!("{:?}", self.0),
                self.render_parent_dom(document),
            )
        }
    }
}

impl<'parent> QueryByRole<'parent> {
    fn get_starting_node_id(&self, document: &DioxusDocument) -> Option<usize> {
        if let Some(parent) = &self.1 {
            parent.get_first_element(document)
        } else {
            Some(document.inner.borrow().root_node().id)
        }
    }

    fn find_first_element_starting_at(
        &self,
        node_id: accesskit::NodeId,
        nodes: &[(accesskit::NodeId, Node)],
    ) -> Option<usize> {
        let (_, node) = nodes.iter().find(|(id, _)| id.0 == node_id.0)?;
        if node.role() == self.0 {
            Some(node_id.0 as usize)
        } else {
            node.children()
                .iter()
                .find_map(|child_id| self.find_first_element_starting_at(*child_id, nodes))
        }
    }

    fn find_all_elements_starting_at(
        &self,
        node_id: accesskit::NodeId,
        nodes: &[(accesskit::NodeId, Node)],
    ) -> Vec<usize> {
        let Some((_, node)) = nodes.iter().find(|(id, _)| id.0 == node_id.0) else {
            return vec![];
        };
        let mut result: Vec<_> = node
            .children()
            .iter()
            .filter_map(|child_id| self.find_first_element_starting_at(*child_id, nodes))
            .collect();
        if node.role() == self.0 {
            result.push(node_id.0 as usize)
        }
        result
    }
}

impl<'parent> ParentableQuery for QueryByRole<'parent> {
    fn with_parent(self, parent: &dyn Query) -> impl CloneableQuery {
        QueryByRole(self.0, Some(parent))
    }
}

impl<'parent> std::fmt::Display for QueryByRole<'parent> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"role="{:?}"#, self.0)
    }
}

impl<'parent> IntoQuery for QueryByRole<'parent> {
    type Query = Self;

    fn into_query(self) -> Self::Query {
        self
    }
}

/// Returns a query selector matching elements with the given ARIA role.
///
/// ```
/// use accesskit::Role;
/// use dioxus::prelude::*;
/// use dioxus_test::{by_role, matchers::{eq, inner_html}, render};
///
/// #[component]
/// fn MyComponent() -> Element {
///     rsx! {
///         button {
///              onclick: |_| {
///                  print!("Clicked!")
///              },
///              "Click me!"
///         }
///     }
/// }
///
/// # async fn test_fn() {
/// let tester = render(MyComponent).build();
/// tester
///     .query(by_role(Role::Button))
///     .click()
///     .await
///     .unwrap();
/// # }
/// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(test_fn());
/// ```
///
/// This attribute is a common convention for marking DOM components with which tests interact. Find
/// more information [here](https://testing-library.com/docs/queries/bytestid/).
pub fn by_role(role: Role) -> impl IntoQuery {
    QueryByRole(role, None)
}
