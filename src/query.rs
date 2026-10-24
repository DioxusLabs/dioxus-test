use crate::TesterError;
use accesskit::Role;
use blitz_dom::{Document as _, SelectorList};
use dioxus_native_dom::DioxusDocument;
use smallvec::SmallVec;
use std::{collections::HashMap, fmt::Write as _, rc::Rc};
use style::dom_apis::{MayUseInvalidation, QueryAll, QueryFirst, query_selector};
use test_that::{
    matcher::{Matcher, MatcherResult},
    matchers::{none, some},
};

/// A value which can be turned into a CSS selector to query the DOM.
///
/// This is implemented for all types which dereference to `str`, including `&str` and `String`.
///
/// One can also select by [testid](https://testing-library.com/docs/queries/bytestid/) using the
/// function [by_testid].
pub trait Query: ToString {
    /// Returns the node ID of the first element in DOM order matching this query.
    fn get_first_element(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId>;

    /// Returns the node IDs of all elements matching this query.
    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<blitz_dom::NodeId>;

    /// Constructs a [TesterError] representing this query failing to match an element.
    fn describe_failure(&self, document: &DioxusDocument) -> TesterError;

    /// Constructs a [TesterError] representing this query unexpectedly matching an element which
    /// is not supposed to exist.
    fn describe_unexpected_element(&self, document: &DioxusDocument) -> TesterError;

    /// Renders the DOM surrounding this query as a pretty-printed string.
    ///
    /// If the query has no parent, this renders the entire DOM of the document. If it has a parent,
    /// and that parent matches an element, then it renders the DOM of that element. If it has a
    /// parent which is not matched, then it returns the output of `render_parent_dom` on the
    /// parent.
    fn render_parent_dom(&self, document: &DioxusDocument) -> String;
}

/// A data type which can be converted into the associated [Query].
///
/// Each concrete query returned by the functions in this model implements this trivially. In
/// addition, string-like types implement this to construct [CssSelectorQuery].
pub trait IntoQuery {
    type Query: ParentableQuery + Clone;

    fn into_query(self) -> Self::Query;
}

/// A [Query] on which one can set a parent query.
///
/// This is a separate trait so that [Query] remains dyn-compatible. All existing query types
/// implement it.
pub trait ParentableQuery: Query {
    /// Constructs a new [ParentableQuery] from this instance with the parent set to the given
    /// value.
    fn with_parent(self, parent: &dyn Query) -> impl ParentableQuery + Clone;
}

/// A query based on an arbitrary CSS selector.
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
    fn get_first_element(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId> {
        let selector_list = self
            .parse_css_selector_to_query(document)
            .expect("Error parsing CSS selector");
        get_first_element_with_selector(document, selector_list, self.1)
    }

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<blitz_dom::NodeId> {
        let selector_list = self
            .parse_css_selector_to_query(document)
            .expect("Error parsing CSS selector");
        get_all_elements_with_selector(document, selector_list, self.1)
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        render_parent_dom(self.1, document)
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

    fn describe_unexpected_element(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.1
            && parent.get_first_element(document).is_none()
        {
            parent.describe_unexpected_element(document)
        } else {
            TesterError::UnexpectedElementWithCssSelector(
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
    fn with_parent(self, parent: &dyn Query) -> impl ParentableQuery + Clone {
        CssSelectorQuery(self.0, Some(parent))
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
/// let tester = render(MyComponent);
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
struct QueryByTestId<'parent>(String, Option<&'parent dyn Query>);

impl<'parent> Query for QueryByTestId<'parent> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId> {
        let selector_list = self.create_selector(document);
        get_first_element_with_selector(document, selector_list, self.1)
    }

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<blitz_dom::NodeId> {
        let selector_list = self.create_selector(document);
        get_all_elements_with_selector(document, selector_list, self.1)
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        render_parent_dom(self.1, document)
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

    fn describe_unexpected_element(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.1
            && parent.get_first_element(document).is_none()
        {
            parent.describe_unexpected_element(document)
        } else {
            TesterError::UnexpectedElementWithTestId(
                self.0.clone(),
                self.render_parent_dom(document),
            )
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
    fn with_parent(self, parent: &dyn Query) -> impl ParentableQuery + Clone {
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

fn get_first_element_with_selector(
    document: &DioxusDocument,
    selector_list: SelectorList,
    parent: Option<&dyn Query>,
) -> Option<blitz_dom::NodeId> {
    let doc_guard = document.inner();
    let start_node = if let Some(parent) = parent {
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

fn get_all_elements_with_selector(
    document: &DioxusDocument,
    selector_list: SelectorList,
    parent: Option<&dyn Query>,
) -> Vec<blitz_dom::NodeId> {
    let doc_guard = document.inner();
    let start_node = if let Some(parent) = parent {
        let Some(parent_node_id) = parent.get_first_element(document) else {
            return vec![];
        };
        let Some(parent_node) = doc_guard.get_node(parent_node_id) else {
            return vec![];
        };
        parent_node
    } else {
        doc_guard.root_node()
    };
    let mut result = SmallVec::new();
    query_selector::<&blitz_dom::Node, QueryAll>(
        start_node,
        &selector_list,
        &mut result,
        MayUseInvalidation::Yes,
    );
    result.into_iter().map(|node| node.id).collect()
}

fn render_parent_dom(parent: Option<&dyn Query>, document: &DioxusDocument) -> String {
    match parent {
        Some(parent) => match parent.get_first_element(document) {
            Some(element) => document
                .inner()
                .get_node(element)
                .expect("Expected to find node")
                .outer_html_pretty(),
            None => parent.render_parent_dom(document),
        },
        None => document.inner().root_element().outer_html_pretty(),
    }
}

/// Returns a query selector matching elements with the given ARIA role.
///
/// ```
/// use dioxus::prelude::*;
/// use dioxus_test::{Role, by_role, matchers::{eq, inner_html}, render};
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
/// let tester = render(MyComponent);
/// tester
///     .query(by_role(Role::Button))
///     .click()
///     .await
///     .unwrap();
/// # }
/// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(test_fn());
/// ```
///
/// Only elements which ae not hidden from assistive technology are matched by default. To include
/// hidden elements, use the methods [`include_hidden`][crate::QueryByAriaProperties::include_hidden]
/// or [`only_hidden`][crate::QueryByAriaProperties::only_hidden]. These only affect elements which
/// are included in the ARIA tree but hidden via the attribute `aria-hidden`, not those which are
/// completely removed from the ARIA tree via CSS selector or the HTML `hidden` attribute.
///
/// Elements which are hidden from the accessibility tree altogether via the `hidden` attribute or
/// the CSS properties `display: none` and `visibility: hidden` cannot be queried with this method.
pub fn by_role(role: Role) -> QueryByAriaProperties<'static> {
    QueryByAriaProperties {
        role: Some(role),
        hidden: Some(false),
        ..Default::default()
    }
}

/// Returns a query selector matching elements whose accessible name is matched by the given
/// matcher.
///
/// See [W3C documentation](https://w3c.github.io/accname/#dfn-accessible-name) for more
/// information on the ARIA accessible name of an element.
///
/// ```
/// use dioxus::prelude::*;
/// use dioxus_test::{Role, by_name, by_testid, matchers::{eq, inner_html}, render};
///
/// #[component]
/// fn MyComponent() -> Element {
///     let mut output = use_signal(|| "");
///     rsx! {
///         button {
///              onclick: move |_| {
///                  output.set("Wrong button clicked")
///              },
///              "Do not click me!"
///         }
///         button {
///              onclick: move |_| {
///                  output.set("Right button clicked")
///              },
///              "Click me!"
///         }
///         div {
///              "data-testid": "output",
///              {output}
///         }
///     }
/// }
///
/// # async fn test_fn() {
/// let tester = render(MyComponent);
/// tester
///     .query(by_name(eq("Click me!")))
///     .click()
///     .await
///     .unwrap();
///
/// tester
///     .query(by_testid("output"))
///     .expect(inner_html(eq("Right button clicked")))
///     .immediately()
///     .unwrap();
/// # }
/// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(test_fn());
/// ```
///
/// You can use the following matchers:
///
/// - [`eq`][crate::matchers::eq] for exact equality,
/// - [`contains_substring`][crate::matchers::contains_substring] for string containment,
/// - [`starts_with`][crate::matchers::starts_with] to match the start of the string,
/// - [`ends_with`][crate::matchers::ends_with] to match the end of the string,
/// - [`matches_regex`][crate::matchers::matches_regex] to match names fully satisfying the
///   given regular expression,
/// - [`contains_regex`][crate::matchers::contains_regex] to match names containing a substring
///   satisfying the given regular expression.
///
/// For case-insensitive matching, invoke the method
/// [`ignoring_ascii_case`][crate::matchers::StrMatcherConfigurator::ignoring_ascii_case]. This
/// is available on all matchers above _except_ `matches_regex` and `contains_regex`.
///
/// ```
/// use dioxus::prelude::*;
/// use dioxus_test::{Role, by_name, by_testid, matchers::{eq, inner_html, StrMatcherConfigurator as _}, render};
///
/// #[component]
/// fn MyComponent() -> Element {
///     let mut output = use_signal(|| "");
///     rsx! {
///         button {
///              onclick: move |_| {
///                  output.set("Button clicked")
///              },
///              "Click me!"
///         }
///         div {
///              "data-testid": "output",
///              {output}
///         }
///     }
/// }
///
/// # async fn test_fn() {
/// let tester = render(MyComponent);
/// tester
///     .query(by_name(eq("click ME!").ignoring_ascii_case()))
///     .click()
///     .await
///     .unwrap();
///
/// tester
///     .query(by_testid("output"))
///     .expect(inner_html(eq("Button clicked")))
///     .immediately()
///     .unwrap();
/// # }
/// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(test_fn());
/// ```
pub fn by_name(label: impl for<'a> Matcher<&'a str> + 'static) -> QueryByAriaProperties<'static> {
    QueryByAriaProperties {
        name: Some(Rc::new(some(label))),
        hidden: Some(false),
        ..Default::default()
    }
}

type OptionStringSliceMatcher = dyn for<'a> Matcher<Option<&'a str>>;

/// A query matching elements based on their [ARIA](https://www.w3.org/TR/wai-aria-1.2/) properties.
#[derive(Clone, Default)]
pub struct QueryByAriaProperties<'parent> {
    role: Option<Role>,
    name: Option<Rc<OptionStringSliceMatcher>>,
    level: Option<Rc<dyn Matcher<usize>>>,
    hidden: Option<bool>,
    expanded: Option<bool>,
    disabled: Option<bool>,
    selected: Option<bool>,
    parent: Option<&'parent dyn Query>,
}

impl<'parent> QueryByAriaProperties<'parent> {
    /// Restricts this query to elements having an accessible name matched by the given matcher.
    ///
    /// See [W3C documentation](https://w3c.github.io/accname/#dfn-accessible-name) for information
    /// on the accessible name of an element.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, by_testid, matchers::{eq, inner_html}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     let mut output = use_signal(|| "");
    ///     rsx! {
    ///         button {
    ///              onclick: move |_| {
    ///                  output.set("Wrong button clicked")
    ///              },
    ///              "Do not click me!"
    ///         }
    ///         button {
    ///              onclick: move |_| {
    ///                  output.set("Right button clicked")
    ///              },
    ///              "Click me!"
    ///         }
    ///         div {
    ///              "data-testid": "output",
    ///              {output}
    ///         }
    ///     }
    /// }
    ///
    /// # async fn test_fn() {
    /// let tester = render(MyComponent);
    /// tester
    ///     .query(by_role(Role::Button).having_name(eq("Click me!")))
    ///     .click()
    ///     .await
    ///     .unwrap();
    ///
    /// tester
    ///     .query(by_testid("output"))
    ///     .expect(inner_html(eq("Right button clicked")))
    ///     .immediately()
    ///     .unwrap();
    /// # }
    /// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(test_fn());
    /// ```
    ///
    /// See [`by_name`] for more information about the matchers one can use with this method.
    pub fn having_name(self, name: impl for<'a> Matcher<&'a str> + 'static) -> Self {
        Self {
            name: Some(Rc::new(some(name))),
            ..self
        }
    }

    /// Restricts this query to elements having no accessible name as defined by the
    /// [W3C specification](https://w3c.github.io/accname/#dfn-accessible-name).
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, by_testid, matchers::{eq, inner_html, len}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         button {}
    ///         button {
    ///              "This button has an accessible name"
    ///         }
    ///     }
    /// }
    ///
    /// # async fn test_fn() {
    /// let tester = render(MyComponent);
    /// tester
    ///     .query_all(by_role(Role::Button).having_no_name())
    ///     .expect(len(eq(1)))
    ///     .await
    ///     .unwrap();
    /// # }
    /// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(test_fn());
    /// ```
    pub fn having_no_name(self) -> Self {
        Self {
            name: Some(Rc::new(none())),
            ..self
        }
    }

    /// Restricts this query to elements having an ARIA level matched by the given matcher.
    ///
    /// The level is one-based, as in the
    /// [`aria-level`](https://www.w3.org/TR/wai-aria-1.2/#aria-level) attribute, and is read from
    /// the level of the element's accessibility node. Elements which have no level never match.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{eq, inner_html}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         h1 { "Title" }
    ///         h2 { "Section" }
    ///         div { role: "heading", "aria-level": "3", "Subsection" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// tester
    ///     .query(by_role(Role::Heading).having_level(eq(2)))
    ///     .expect(inner_html(eq("Section")))
    ///     .immediately()
    ///     .unwrap();
    /// tester
    ///     .query(by_role(Role::Heading).having_level(eq(3)))
    ///     .expect(inner_html(eq("Subsection")))
    ///     .immediately()
    ///     .unwrap();
    /// ```
    ///
    /// You can use the following matchers:
    ///
    /// - [`eq`][crate::matchers::eq] for exact equality,
    /// - [`gt`][crate::matchers::gt] for levels strictly greater than the given value,
    /// - [`ge`][crate::matchers::ge] for levels greater than or equal to the given value,
    /// - [`lt`][crate::matchers::lt] for levels strictly less than the given value,
    /// - [`le`][crate::matchers::le] for levels less than or equal to the given value,
    /// - [`in_range`][crate::matchers::in_range] for levels within a range, such as
    ///   `in_range(2..=4)`.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{in_range, le}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         h1 { "One" }
    ///         h2 { "Two" }
    ///         h3 { "Three" }
    ///         h4 { "Four" }
    ///         h5 { "Five" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// assert_eq!(
    ///     tester.query_all(by_role(Role::Heading).having_level(in_range(2..=4))).immediately().len(),
    ///     3
    /// );
    /// assert_eq!(
    ///     tester.query_all(by_role(Role::Heading).having_level(le(2))).immediately().len(),
    ///     2
    /// );
    /// ```
    pub fn having_level(self, level: impl Matcher<usize> + 'static) -> Self {
        Self {
            level: Some(Rc::new(level)),
            ..self
        }
    }

    /// Restricts this query to elements which are hidden from assistive technology by having the
    /// attribute `aria-hidden="true"`.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{eq, inner_html}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         button { "Visible button" }
    ///         button { "aria-hidden": "true", "Hidden button" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// tester
    ///     .query(by_role(Role::Button).only_hidden())
    ///     .expect(inner_html(eq("Hidden button")))
    ///     .immediately()
    ///     .unwrap();
    /// tester
    ///     .query(by_role(Role::Button).having_name(eq("Visible button")).only_hidden())
    ///     .expect_no_matching_element()
    ///     .immediately()
    ///     .unwrap();
    /// ```
    ///
    /// An element with `aria-hidden="false"` is not hidden. Elements hidden through CSS, such as
    /// with `display: none` or `visibility: hidden`, are not part of the accessibility tree at all,
    /// so no query by role matches them, whatever value is given here.
    ///
    /// Child elements of an element with `aria-hidden="true"` are also hidden.
    pub fn only_hidden(self) -> Self {
        Self {
            hidden: Some(true),
            ..self
        }
    }

    /// Includes in this query elements which are hidden from assistive technology by having the
    /// attribute `aria-hidden="true"`.
    ///
    /// Both hidden and non-hidden elements are matched.
    ///
    /// See the [W3C documentation](https://www.w3.org/TR/wai-aria-1.2/#aria-hidden) for details on
    /// the attribute.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{eq, inner_html, len}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         button { "Visible button" }
    ///         button { "aria-hidden": "true", "Hidden button" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// tester
    ///     .query_all(by_role(Role::Button).include_hidden())
    ///     .expect(len(eq(2)))
    ///     .immediately()
    ///     .unwrap();
    /// ```
    ///
    /// An element with `aria-hidden="false"` is not hidden. Elements hidden through CSS, such as
    /// with `display: none` or `visibility: hidden`, are not part of the accessibility tree at all,
    /// so no query by role matches them, whatever value is given here.
    ///
    /// Child elements of an element with `aria-hidden="true"` are also hidden.
    pub fn include_hidden(self) -> Self {
        Self {
            hidden: None,
            ..self
        }
    }

    /// Restricts this query to elements which are expanded or collapsed, as given by the
    /// attribute `aria-expanded`.
    ///
    /// With `expanded(true)`, the query only matches elements with `aria-expanded="true"`. With
    /// `expanded(false)`, it only matches elements with `aria-expanded="false"`. By default, a
    /// query matches elements regardless of this attribute. See the
    /// [W3C documentation](https://www.w3.org/TR/wai-aria-1.2/#aria-expanded) for details on the
    /// attribute.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{eq, inner_html}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         button { "aria-expanded": "false", "Collapsed button" }
    ///         button { "aria-expanded": "true", "Expanded button" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// tester
    ///     .query(by_role(Role::Button).expanded(true))
    ///     .expect(inner_html(eq("Expanded button")))
    ///     .immediately()
    ///     .unwrap();
    /// tester
    ///     .query(by_role(Role::Button).expanded(false))
    ///     .expect(inner_html(eq("Collapsed button")))
    ///     .immediately()
    ///     .unwrap();
    /// ```
    ///
    /// An element without an `aria-expanded` attribute is neither expanded nor collapsed, so it
    /// is matched by neither `expanded(true)` nor `expanded(false)`.
    pub fn expanded(self, expanded: bool) -> Self {
        Self {
            expanded: Some(expanded),
            ..self
        }
    }

    /// Restricts this query to elements which are, or are not, disabled, as given by the
    /// attribute `aria-disabled`.
    ///
    /// With `disabled(true)`, the query only matches elements with `aria-disabled="true"`. With
    /// `disabled(false)`, it only matches elements which are _not_ disabled, including those
    /// without an `aria-disabled` attribute. By default, a query matches elements regardless of
    /// whether they are disabled. See the
    /// [W3C documentation](https://www.w3.org/TR/wai-aria-1.2/#aria-disabled) for details on the
    /// attribute.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{eq, inner_html}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         button { "Enabled button" }
    ///         button { "aria-disabled": "true", "Disabled button" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// tester
    ///     .query(by_role(Role::Button).disabled(true))
    ///     .expect(inner_html(eq("Disabled button")))
    ///     .immediately()
    ///     .unwrap();
    /// tester
    ///     .query(by_role(Role::Button).disabled(false))
    ///     .expect(inner_html(eq("Enabled button")))
    ///     .immediately()
    ///     .unwrap();
    /// ```
    pub fn disabled(self, disabled: bool) -> Self {
        Self {
            disabled: Some(disabled),
            ..self
        }
    }

    /// Restricts this query to elements which are selected or not selected, as given by the
    /// attribute `aria-selected`.
    ///
    /// With `selected(true)`, the query only matches elements with `aria-selected="true"`. With
    /// `selected(false)`, it only matches elements with `aria-selected="false"`. By default, a
    /// query matches elements regardless of this attribute. See the
    /// [W3C documentation](https://www.w3.org/TR/wai-aria-1.2/#aria-selected) for details on the
    /// attribute.
    ///
    /// ```
    /// use dioxus::prelude::*;
    /// use dioxus_test::{Role, by_role, matchers::{eq, inner_html}, render};
    ///
    /// #[component]
    /// fn MyComponent() -> Element {
    ///     rsx! {
    ///         div { role: "tab", "aria-selected": "false", "Unselected tab" }
    ///         div { role: "tab", "aria-selected": "true", "Selected tab" }
    ///     }
    /// }
    ///
    /// let tester = render(MyComponent);
    /// tester
    ///     .query(by_role(Role::Tab).selected(true))
    ///     .expect(inner_html(eq("Selected tab")))
    ///     .immediately()
    ///     .unwrap();
    /// tester
    ///     .query(by_role(Role::Tab).selected(false))
    ///     .expect(inner_html(eq("Unselected tab")))
    ///     .immediately()
    ///     .unwrap();
    /// ```
    ///
    /// An element without an `aria-selected` attribute is neither selected nor unselected, so it
    /// is matched by neither `selected(true)` nor `selected(false)`.
    pub fn selected(self, selected: bool) -> Self {
        Self {
            selected: Some(selected),
            ..self
        }
    }
}

impl<'parent> Query for QueryByAriaProperties<'parent> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId> {
        let aria_tree = AriaTree::for_document(document);
        let starting_node_id = self.get_starting_node_id(document)?;
        self.find_first_element_starting_at(
            accesskit::NodeId(starting_node_id.as_u64()),
            &aria_tree,
            false,
        )
    }

    fn get_all_elements(&self, document: &DioxusDocument) -> Vec<blitz_dom::NodeId> {
        let aria_tree = AriaTree::for_document(document);
        let Some(starting_node_id) = self.get_starting_node_id(document) else {
            return vec![];
        };
        self.find_all_elements_starting_at(
            accesskit::NodeId(starting_node_id.as_u64()),
            &aria_tree,
            false,
        )
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        render_parent_dom(self.parent, document)
    }

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.parent
            && parent.get_first_element(document).is_none()
        {
            parent.describe_failure(document)
        } else {
            TesterError::NoSuchElementWithAriaProperties(
                self.describe_self(),
                self.render_parent_dom(document),
            )
        }
    }

    fn describe_unexpected_element(&self, document: &DioxusDocument) -> TesterError {
        if let Some(parent) = self.parent
            && parent.get_first_element(document).is_none()
        {
            parent.describe_unexpected_element(document)
        } else {
            TesterError::UnexpectedElementWithAriaProperties(
                self.describe_self(),
                self.render_parent_dom(document),
            )
        }
    }
}

impl<'parent> QueryByAriaProperties<'parent> {
    fn get_starting_node_id(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId> {
        if let Some(parent) = &self.parent {
            parent.get_first_element(document)
        } else {
            Some(document.inner.borrow().root_node().id)
        }
    }

    fn find_first_element_starting_at(
        &self,
        node_id: accesskit::NodeId,
        aria_tree: &AriaTree,
        ancestor_hidden: bool,
    ) -> Option<blitz_dom::NodeId> {
        let node = aria_tree.get_node(node_id)?;
        if self.element_matches(node, node.is_hidden() || ancestor_hidden) {
            Some(blitz_dom::NodeId::from_u64(node_id.0))
        } else {
            node.children().iter().find_map(|child_id| {
                self.find_first_element_starting_at(
                    *child_id,
                    aria_tree,
                    ancestor_hidden || node.is_hidden(),
                )
            })
        }
    }

    fn find_all_elements_starting_at(
        &self,
        node_id: accesskit::NodeId,
        aria_tree: &AriaTree,
        ancestor_hidden: bool,
    ) -> Vec<blitz_dom::NodeId> {
        let Some(node) = aria_tree.get_node(node_id) else {
            return vec![];
        };
        let mut result: Vec<_> = node
            .children()
            .iter()
            .flat_map(|child_id| {
                self.find_all_elements_starting_at(
                    *child_id,
                    aria_tree,
                    ancestor_hidden || node.is_hidden(),
                )
            })
            .collect();
        if self.element_matches(node, node.is_hidden() || ancestor_hidden) {
            result.push(blitz_dom::NodeId::from_u64(node_id.0))
        }
        result
    }

    fn element_matches(&self, node: &accesskit::Node, is_hidden: bool) -> bool {
        self.role.is_none_or(|role| role == node.role())
            && self
                .name
                .as_ref()
                .is_none_or(|name| name.matches(&node.label()).is_match())
            && self.level.as_ref().is_none_or(|level| {
                // accesskit levels are zero-based, while ARIA levels are one-based.
                // See https://docs.rs/accesskit/latest/accesskit/struct.Node.html#method.level
                node.level()
                    .is_some_and(|actual| level.matches(&(actual + 1)).is_match())
            })
            && self.hidden.is_none_or(|hidden| is_hidden == hidden)
            && self
                .expanded
                .is_none_or(|expanded| node.is_expanded() == Some(expanded))
            && self
                .disabled
                .is_none_or(|disabled| node.is_disabled() == disabled)
            && self
                .selected
                .is_none_or(|selected| node.is_selected() == Some(selected))
    }

    fn describe_self(&self) -> String {
        let mut result = String::new();
        if let Some(role) = self.role {
            write!(result, "role {role:?}").unwrap(); // Infallible
            if let Some(name) = &self.name {
                write!(
                    result,
                    " having accessible name {}",
                    name.describe(MatcherResult::Match)
                )
                .unwrap(); // Infallible
            }
        } else if let Some(name) = &self.name {
            write!(result, "name {}", name.describe(MatcherResult::Match)).unwrap(); // Infallible
        }
        if let Some(level) = &self.level {
            write!(
                result,
                " having ARIA level {}",
                level.describe(MatcherResult::Match)
            )
            .unwrap(); // Infallible
        }
        match self.hidden {
            Some(true) => result.push_str(" only hidden"),
            Some(false) => {}
            None => result.push_str(" including hidden"),
        }
        match self.expanded {
            Some(true) => result.push_str(" being expanded"),
            Some(false) => result.push_str(" being collapsed"),
            None => {}
        }
        match self.disabled {
            Some(true) => result.push_str(" being disabled"),
            Some(false) => result.push_str(" not being disabled"),
            None => {}
        }
        match self.selected {
            Some(true) => result.push_str(" being selected"),
            Some(false) => result.push_str(" being unselected"),
            None => {}
        }
        result
    }
}

impl<'parent> ParentableQuery for QueryByAriaProperties<'parent> {
    fn with_parent(self, parent: &dyn Query) -> impl ParentableQuery + Clone {
        QueryByAriaProperties {
            parent: Some(parent),
            ..self
        }
    }
}

impl<'parent> std::fmt::Display for QueryByAriaProperties<'parent> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(role) = self.role {
            write!(f, r#"role="{role:?}""#)?;
            if let Some(name) = &self.name {
                write!(f, r#" having name {}"#, name.describe(MatcherResult::Match))?;
            }
        } else if let Some(name) = &self.name {
            write!(f, r#"name {}"#, name.describe(MatcherResult::Match))?;
        }
        if let Some(level) = &self.level {
            write!(
                f,
                r#" having level {}"#,
                level.describe(MatcherResult::Match)
            )?;
        }
        match self.hidden {
            Some(true) => write!(f, " only hidden")?,
            Some(false) => {}
            None => write!(f, " including hidden")?,
        }
        match self.expanded {
            Some(true) => write!(f, " being expanded")?,
            Some(false) => write!(f, " being collapsed")?,
            None => {}
        }
        match self.disabled {
            Some(true) => write!(f, " being disabled")?,
            Some(false) => write!(f, " not being disabled")?,
            None => {}
        }
        match self.selected {
            Some(true) => write!(f, " being selected")?,
            Some(false) => write!(f, " being unselected")?,
            None => {}
        }
        Ok(())
    }
}

impl<'parent> IntoQuery for QueryByAriaProperties<'parent> {
    type Query = Self;

    fn into_query(self) -> Self::Query {
        self
    }
}

#[derive(Clone)]
#[doc(hidden)]
pub struct NthElementQuery<Q: Clone> {
    full_query: Q,
    n: usize,
}

impl<Q: Clone> NthElementQuery<Q> {
    pub(crate) fn new(full_query: Q, n: usize) -> Self {
        Self { full_query, n }
    }
}

impl<Q: Query + Clone + std::fmt::Display> Query for NthElementQuery<Q> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId> {
        self.full_query
            .get_all_elements(document)
            .get(self.n)
            .cloned()
    }

    fn get_all_elements(&self, _: &DioxusDocument) -> Vec<blitz_dom::NodeId> {
        unimplemented!("This query type is only for single-element queries")
    }

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError {
        self.full_query.describe_failure(document)
    }

    fn describe_unexpected_element(&self, document: &DioxusDocument) -> TesterError {
        self.full_query.describe_unexpected_element(document)
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        self.full_query.render_parent_dom(document)
    }
}

impl<Q: std::fmt::Display + Clone> std::fmt::Display for NthElementQuery<Q> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (element {})", self.full_query, self.n)
    }
}

#[derive(Clone)]
#[doc(hidden)]
pub struct LastElementQuery<Q: Clone> {
    full_query: Q,
}

impl<Q: Clone> LastElementQuery<Q> {
    pub(crate) fn new(full_query: Q) -> Self {
        Self { full_query }
    }
}

impl<Q: Query + Clone + std::fmt::Display> Query for LastElementQuery<Q> {
    fn get_first_element(&self, document: &DioxusDocument) -> Option<blitz_dom::NodeId> {
        self.full_query.get_all_elements(document).last().cloned()
    }

    fn get_all_elements(&self, _: &DioxusDocument) -> Vec<blitz_dom::NodeId> {
        unimplemented!("This query type is only for single-element queries")
    }

    fn describe_failure(&self, document: &DioxusDocument) -> TesterError {
        self.full_query.describe_failure(document)
    }

    fn describe_unexpected_element(&self, document: &DioxusDocument) -> TesterError {
        self.full_query.describe_unexpected_element(document)
    }

    fn render_parent_dom(&self, document: &DioxusDocument) -> String {
        self.full_query.render_parent_dom(document)
    }
}

impl<Q: std::fmt::Display + Clone> std::fmt::Display for LastElementQuery<Q> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (last element)", self.full_query)
    }
}

/// A representation of the ARIA tree constructed from a document.
struct AriaTree {
    nodes_by_node_id: HashMap<accesskit::NodeId, accesskit::Node>,
}

impl AriaTree {
    /// Constructs a new [`AriaTree`] from the data in the DOM of the given [`DioxusDocument`].
    pub(super) fn for_document(document: &DioxusDocument) -> Self {
        let tree = document.inner.borrow().build_accessibility_tree();
        let nodes_by_node_id = tree.nodes.into_iter().collect();
        Self { nodes_by_node_id }
    }

    /// Returns the accesskit [Node] with the given [NodeId], if it exists.
    pub(super) fn get_node(&self, node_id: accesskit::NodeId) -> Option<&accesskit::Node> {
        self.nodes_by_node_id.get(&node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::by_role;
    use crate::{by_name, matchers::inner_html, render};
    use accesskit::Role;
    use dioxus::prelude::*;
    use test_that::prelude::*;

    #[test]
    fn by_role_display_format_contains_name() -> TestResult<()> {
        let query = by_role(Role::Button).having_name(eq("A button"));

        verify_that!(format!("{query}"), contains_substring("A button"))
    }

    #[test]
    fn failure_message_for_query_by_role_includes_name() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {}
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).having_name(eq("A button")))
            .immediately();

        verify_that!(result, err(displays_as(contains_substring("A button"))))
    }

    #[test]
    fn by_role_display_format_contains_level() -> TestResult<()> {
        let query = by_role(Role::Heading).having_level(eq(3));

        verify_that!(
            format!("{query}"),
            eq(r#"role="Heading" having level is equal to 3"#)
        )
    }

    #[test]
    fn having_name_with_empty_string_does_not_match_unnamed_element() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button {}
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Button).having_name(eq("")))
            .expect_no_matching_element()
            .immediately()
    }

    #[test]
    fn having_no_name_does_not_match_named_element() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "A button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Button).having_no_name())
            .expect_no_matching_element()
            .immediately()
    }

    #[test]
    fn having_name_gets_name_from_aria_label() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "aria-label": "A button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).having_name(eq("A button")))
            .expect(len(eq(1)))
            .immediately()
    }

    #[test]
    fn failure_message_for_query_by_role_includes_level() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {}
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Heading).having_level(eq(3)))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Heading having ARIA level is equal to 3"
            )))
        )
    }

    #[test]
    fn having_level_matches_heading_by_tag() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                h1 { "One" }
                h2 { "Two" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Heading).having_level(eq(2)))
            .expect(inner_html(eq("Two")))
            .immediately()
    }

    #[test]
    fn having_level_matches_explicit_aria_level_over_tag() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                h1 { "aria-level": "4", "One" }
                h2 { "Two" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Heading).having_level(eq(4)))
            .expect(inner_html(eq("One")))
            .immediately()
    }

    #[test]
    fn having_level_does_not_match_element_without_level() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Click" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Button).having_level(ge(0)))
            .expect_no_matching_element()
            .immediately()
    }

    #[test]
    fn having_level_combines_with_name() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                h1 { "First Title" }
                h2 { "Second Title" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(
                by_role(Role::Heading)
                    .having_name(contains_substring("Title"))
                    .having_level(eq(2)),
            )
            .expect(inner_html(eq("Second Title")))
            .immediately()
    }

    #[test]
    fn by_role_display_format_contains_only_hidden() -> TestResult<()> {
        let query = by_role(Role::Button).only_hidden();

        verify_that!(format!("{query}"), eq(r#"role="Button" only hidden"#))
    }

    #[test]
    fn by_role_display_format_contains_include_hidden() -> TestResult<()> {
        let query = by_role(Role::Button).include_hidden();

        verify_that!(format!("{query}"), eq(r#"role="Button" including hidden"#))
    }

    #[test]
    fn failure_message_for_query_by_role_includes_only_hidden() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Visible button" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).only_hidden())
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Button only hidden"
            )))
        )
    }

    #[test]
    fn failure_message_for_query_by_role_includes_including_hidden() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {}
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).include_hidden())
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Button including hidden"
            )))
        )
    }

    #[test]
    fn by_role_matches_only_element_with_aria_hidden_true_when_only_hidden_called()
    -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Visible button" }
                button { "aria-hidden": "false", "Not hidden button" }
                button { "aria-hidden": "true", "Hidden button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).only_hidden())
            .expect(len(eq(1)))
            .immediately()?;
        tester
            .query(by_role(Role::Button).only_hidden())
            .expect(inner_html(eq("Hidden button")))
            .immediately()
    }

    #[test]
    fn by_role_matches_hidden_and_non_hidden_elements_when_include_hidden_called()
    -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Visible button" }
                button { "aria-hidden": "false", "Not hidden button" }
                button { "aria-hidden": "true", "Hidden button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).include_hidden())
            .expect(len(eq(3)))
            .immediately()
    }

    #[test]
    fn by_role_does_not_match_descendant_of_element_with_aria_hidden_true_by_default()
    -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Visible button" }
                div {
                    "aria-hidden": "true",
                    button { "Button inside hidden element" }
                }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button))
            .expect(len(eq(1)))
            .immediately()
    }

    #[test]
    fn by_role_matches_descendant_of_element_with_aria_hidden_true_when_only_hidden_called()
    -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Visible button" }
                div {
                    "aria-hidden": "true",
                    button { "Button inside hidden element" }
                }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Button).only_hidden())
            .expect(inner_html(eq("Button inside hidden element")))
            .immediately()
    }

    #[test]
    fn by_role_display_format_contains_expanded_true() -> TestResult<()> {
        let query = by_role(Role::Button).expanded(true);

        verify_that!(format!("{query}"), eq(r#"role="Button" being expanded"#))
    }

    #[test]
    fn by_role_display_format_contains_expanded_false() -> TestResult<()> {
        let query = by_role(Role::Button).expanded(false);

        verify_that!(format!("{query}"), eq(r#"role="Button" being collapsed"#))
    }

    #[test]
    fn failure_message_for_query_by_role_includes_expanded_true() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "aria-expanded": "false", "Collapsed button" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).expanded(true))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Button being expanded"
            )))
        )
    }

    #[test]
    fn failure_message_for_query_by_role_includes_expanded_false() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "aria-expanded": "true", "Expanded button" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).expanded(false))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Button being collapsed"
            )))
        )
    }

    #[test]
    fn expanded_true_matches_only_element_with_aria_expanded_true() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "No attribute button" }
                button { "aria-expanded": "false", "Collapsed button" }
                button { "aria-expanded": "true", "Expanded button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).expanded(true))
            .expect(len(eq(1)))
            .immediately()?;
        tester
            .query(by_role(Role::Button).expanded(true))
            .expect(inner_html(eq("Expanded button")))
            .immediately()
    }

    #[test]
    fn expanded_false_matches_only_element_with_aria_expanded_false() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "No attribute button" }
                button { "aria-expanded": "false", "Collapsed button" }
                button { "aria-expanded": "true", "Expanded button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).expanded(false))
            .expect(len(eq(1)))
            .immediately()?;
        tester
            .query(by_role(Role::Button).expanded(false))
            .expect(inner_html(eq("Collapsed button")))
            .immediately()
    }

    #[test]
    fn by_role_matches_regardless_of_expanded_by_default() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "No attribute button" }
                button { "aria-expanded": "false", "Collapsed button" }
                button { "aria-expanded": "true", "Expanded button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button))
            .expect(len(eq(3)))
            .immediately()
    }

    #[test]
    fn expanded_combines_with_only_hidden() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "aria-expanded": "true", "aria-hidden": "true", "Hidden button" }
                button { "aria-expanded": "true", "Visible button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(by_role(Role::Button).expanded(true).only_hidden())
            .expect(inner_html(eq("Hidden button")))
            .immediately()
    }

    #[test]
    fn by_role_display_format_contains_disabled_true() -> TestResult<()> {
        let query = by_role(Role::Button).disabled(true);

        verify_that!(format!("{query}"), eq(r#"role="Button" being disabled"#))
    }

    #[test]
    fn by_role_display_format_contains_disabled_false() -> TestResult<()> {
        let query = by_role(Role::Button).disabled(false);

        verify_that!(
            format!("{query}"),
            eq(r#"role="Button" not being disabled"#)
        )
    }

    #[test]
    fn failure_message_for_query_by_role_includes_disabled_true() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "Enabled button" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).disabled(true))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Button being disabled"
            )))
        )
    }

    #[test]
    fn failure_message_for_query_by_role_includes_disabled_false() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "aria-disabled": "true", "Disabled button" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Button).disabled(false))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Button not being disabled"
            )))
        )
    }

    #[test]
    fn disabled_true_matches_only_element_with_aria_disabled_true() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "No attribute button" }
                button { "aria-disabled": "false", "Enabled button" }
                button { "aria-disabled": "true", "Disabled button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).disabled(true))
            .expect(len(eq(1)))
            .immediately()?;
        tester
            .query(by_role(Role::Button).disabled(true))
            .expect(inner_html(eq("Disabled button")))
            .immediately()
    }

    #[test]
    fn disabled_false_matches_elements_without_aria_disabled_true() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "No attribute button" }
                button { "aria-disabled": "false", "Enabled button" }
                button { "aria-disabled": "true", "Disabled button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button).disabled(false))
            .expect(len(eq(2)))
            .immediately()
    }

    #[test]
    fn by_role_matches_regardless_of_disabled_by_default() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "No attribute button" }
                button { "aria-disabled": "false", "Enabled button" }
                button { "aria-disabled": "true", "Disabled button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Button))
            .expect(len(eq(3)))
            .immediately()
    }

    #[test]
    fn disabled_combines_with_name() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button { "aria-disabled": "true", "First button" }
                button { "aria-disabled": "true", "Second button" }
                button { "Second button" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(
                by_role(Role::Button)
                    .having_name(eq("Second button"))
                    .disabled(true),
            )
            .expect(len(eq(1)))
            .immediately()
    }

    #[test]
    fn by_role_display_format_contains_selected_true() -> TestResult<()> {
        let query = by_role(Role::Tab).selected(true);

        verify_that!(format!("{query}"), eq(r#"role="Tab" being selected"#))
    }

    #[test]
    fn by_role_display_format_contains_selected_false() -> TestResult<()> {
        let query = by_role(Role::Tab).selected(false);

        verify_that!(format!("{query}"), eq(r#"role="Tab" being unselected"#))
    }

    #[test]
    fn failure_message_for_query_by_role_includes_selected_true() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div { role: "tab", "aria-selected": "false", "Unselected tab" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Tab).selected(true))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Tab being selected"
            )))
        )
    }

    #[test]
    fn failure_message_for_query_by_role_includes_selected_false() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div { role: "tab", "aria-selected": "true", "Selected tab" }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_role(Role::Tab).selected(false))
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                "No such element with role Tab being unselected"
            )))
        )
    }

    #[test]
    fn selected_true_matches_only_element_with_aria_selected_true() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div { role: "tab", "No attribute tab" }
                div { role: "tab", "aria-selected": "false", "Unselected tab" }
                div { role: "tab", "aria-selected": "true", "Selected tab" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Tab).selected(true))
            .expect(len(eq(1)))
            .immediately()?;
        tester
            .query(by_role(Role::Tab).selected(true))
            .expect(inner_html(eq("Selected tab")))
            .immediately()
    }

    #[test]
    fn selected_false_matches_only_element_with_aria_selected_false() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div { role: "tab", "No attribute tab" }
                div { role: "tab", "aria-selected": "false", "Unselected tab" }
                div { role: "tab", "aria-selected": "true", "Selected tab" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Tab).selected(false))
            .expect(len(eq(1)))
            .immediately()?;
        tester
            .query(by_role(Role::Tab).selected(false))
            .expect(inner_html(eq("Unselected tab")))
            .immediately()
    }

    #[test]
    fn by_role_matches_regardless_of_selected_by_default() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div { role: "tab", "No attribute tab" }
                div { role: "tab", "aria-selected": "false", "Unselected tab" }
                div { role: "tab", "aria-selected": "true", "Selected tab" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query_all(by_role(Role::Tab))
            .expect(len(eq(3)))
            .immediately()
    }

    #[test]
    fn selected_combines_with_name() -> crate::Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div { role: "tab", "aria-selected": "true", "First tab" }
                div { role: "tab", "aria-selected": "true", "Second tab" }
                div { role: "tab", "aria-selected": "false", "Second tab (not selected)" }
            }
        }
        let tester = render(TestComponent);

        tester
            .query(
                by_role(Role::Tab)
                    .having_name(contains_substring("Second tab"))
                    .selected(true),
            )
            .expect(inner_html(eq("Second tab")))
            .immediately()
    }

    #[test]
    fn failure_message_for_query_by_name_includes_just_name() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {}
        }
        let tester = render(TestComponent);

        let result = tester.query(by_name(eq("A button"))).immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                r#"No such element with name has a value which is equal to "A button""#
            )))
        )
    }

    #[test]
    fn failure_message_for_unexpected_element_query_by_name_includes_just_name() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                button {
                    "A button"
                }
            }
        }
        let tester = render(TestComponent);

        let result = tester
            .query(by_name(eq("A button")))
            .expect_no_matching_element()
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(
                r#"Unexpected element with name has a value which is equal to "A button""#
            )))
        )
    }

    #[test]
    fn by_name_display_format_contains_name() -> TestResult<()> {
        let query = by_name(eq("A button"));

        verify_that!(
            format!("{query}"),
            contains_substring(r#"is equal to "A button""#)
        )
    }
}
