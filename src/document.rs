use crate::{
    condition::{AllElementsCondition, ElementCondition},
    element::{NodeId, ResolvedElement},
    result::{ErrorBuilder, TesterError},
};
use blitz_dom::{Document as _, SelectorList};
use dioxus_core::{Element, VirtualDom};
use dioxus_native_dom::{DioxusDocument, DocumentConfig};
use std::{cell::RefCell, rc::Rc, time::Duration};
use tokio::time::{error::Elapsed, timeout};

/// The maximum time [DocumentTester] will wait for new events when running [DocumentTester::pump]
/// before concluding that no new events are forthcoming.
// TODO: Make this configurable.
const PUMP_TIMEOUT: Duration = Duration::from_millis(1000);

/// Returns a new [DocumentTester] resulting from rendering the given [Element].
pub fn render(element: fn() -> Element) -> DocumentTester {
    DocumentTester::from_element(element)
}

/// A wrapper which allows querying and interacting with a DOM in Dioxus tests.
pub struct DocumentTester {
    document: Rc<RefCell<DioxusDocument>>,
    now: f64,
    window_size: Option<(u32, u32)>,
}

impl DocumentTester {
    /// Constructs a new instance by rendering the given `element`.
    pub fn from_element(element: fn() -> Element) -> Self {
        let virtual_dom = VirtualDom::new(element);
        let document = Rc::new(RefCell::new(DioxusDocument::new(
            virtual_dom,
            DocumentConfig {
                style_threading: blitz_dom::StyleThreading::Sequential,
                ..Default::default()
            },
        )));
        Self {
            document,
            now: 0.0,
            window_size: None,
        }
    }

    /// Constructs a new instance from the given [VirtualDom].
    pub fn from_virtual_dom(virtual_dom: VirtualDom) -> Self {
        let document = Rc::new(RefCell::new(DioxusDocument::new(
            virtual_dom,
            DocumentConfig {
                style_threading: blitz_dom::StyleThreading::Sequential,
                ..Default::default()
            },
        )));
        Self {
            document,
            now: 0.0,
            window_size: None,
        }
    }

    /// Adds the given context to the root of this tester's virtual DOM.
    ///
    /// The context is available to all elements within the DOM.
    ///
    /// See [Dioxus documentation](https://dioxuslabs.com/learn/0.7/essentials/basics/context) for
    /// more information on context.
    pub fn with_root_context<T: Clone + 'static>(self, context: T) -> Self {
        self.document.borrow().vdom.provide_root_context(context);
        self
    }

    /// Sets the size of the window in pixels to which this DOM will virtually render.
    pub fn with_window_size(mut self, width: u32, height: u32) -> Self {
        self.window_size = Some((width, height));
        self
    }

    /// Performs a layout and build for the DOM managed by this tester.
    ///
    /// This method must be invoked before querying any elements.
    pub fn build(self) -> Self {
        let mut document = self.document.borrow_mut();
        document.inner_mut().viewport_mut().window_size = self.window_size.unwrap_or((500, 800));
        document.initial_build();
        document.inner_mut().resolve(self.now);
        drop(document);
        self
    }

    /// Resolve a single round of asynchronous operations via the async runtime and the Dioxus
    /// runtime.
    ///
    /// This performs a single round of one of the following:
    ///
    /// - Allow the runtime to process any events which have been dispatch, invoking the event
    ///   handlers.
    /// - Resolve a single round of async operations external to the Dioxus runtime, such as
    ///   network requests.
    ///
    /// For example, if you have a button whose event handler initiates a network request, then a
    /// single call to this method will invoke the event handler and run it until it performs the
    /// network request. A second invocation of this method will resolve the network request and
    /// continue the event handler from that point.
    ///
    /// ```no_run
    /// # use dioxus::prelude::*;
    /// # #[component]
    /// # fn AComponent() -> Element { rsx! { } }
    /// # async fn run_test() -> Result<(), Box<dyn std::error::Error>> {
    /// # let tester = dioxus_test::render(AComponent).build();
    /// tester.query("make-request-button").click().await;
    ///
    /// tester.pump().await?; // React to the click
    /// // Assert on the state of the UI while the network request is in flight.
    ///
    /// tester.pump().await?; // Receive the server response
    /// // Assert on the state of the UI after the response is received and the UI has been
    /// // rerendered.
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// If this method is invoked with no pending asynchronous operations, then it times out after
    /// one second and returns `Err(Elapsed)`.
    // Carrying the exclusively borrowed reference to DioxusDocument through the await point is
    // unavoidable. We need an exclusive reference to the VirtualDom to invoke wait_for_work(),
    // which is precisely the async method. This should be no problem as long as the test doesn't
    // try multiple concurrent invocations of pump().
    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn pump(&self) -> Result<(), Elapsed> {
        let mut document = self.document.borrow_mut();
        timeout(PUMP_TIMEOUT, document.vdom.wait_for_work()).await?;
        while document.poll(None) {}
        Ok(())
    }

    /// Advance the internal clock by the given [Duration].
    ///
    /// This advances any CSS animations which may be in progress and recalculates the layout.
    pub async fn advance_time(&mut self, duration: Duration) {
        self.now += duration.as_secs_f64();
        let mut document = self.document.borrow_mut();
        document.inner_mut().resolve(self.now);
    }

    /// Returns an element referencing the root DOM node managed by this tester.
    ///
    /// This allows interacting with and asserting on the root element. However, there is no support
    /// for awaiting expectations. If the test must await an expectation on the root element use
    /// [Self::query] with the CSS selector `:root`.
    pub fn root(&self) -> ResolvedElement {
        ResolvedElement {
            document: self.document.clone(),
            node_id: NodeId::Root,
        }
    }

    /// Immediately returns the first element in the DOM satisfying the given query.
    ///
    /// If no such element already exists on the DOM, then this returns an error.
    ///
    /// Returns an error if the Query contains a syntactically invalid CSS selector.
    pub(crate) fn get_element(&self, query: &SelectorList) -> Option<usize> {
        self.document.borrow().inner().query_selector_raw(query)
    }

    /// Immediately returns all already elements in the DOM satisfying the given query.
    ///
    /// Returns an error if the Query contains a syntactically invalid CSS selector.
    pub(crate) fn get_elements(&self, query: &SelectorList) -> Vec<usize> {
        self.document
            .borrow()
            .inner()
            .query_selector_all_raw(query)
            .to_vec()
    }

    /// Returns a representation of first element in the DOM satisfying the given query.
    ///
    /// The query can be anything which dereferences to a `str`, including `&str` and `String`. This
    /// method then interprets it as a CSS selector. Alternatively, one can select by testid with
    /// [by_testid].
    ///
    /// The test can:
    ///
    /// - await the matching element by driving the event loop until it appears,
    /// - immediately resolve the element in order to assert on or interact with it, or
    /// - make an assertion and drive the event loop until that assertion to be true.
    ///
    /// See [ElementCondition] for more.
    ///
    /// ```rust
    /// # use dioxus::prelude::*;
    /// # use dioxus_test::{*, matchers::*};
    /// #[component]
    /// fn AComponent() -> Element {
    ///    let mut click_count = use_signal(|| 0);
    ///    rsx! {
    ///        button {
    ///            onclick: move |_| click_count += 1,
    ///            "Click me!"
    ///        }
    ///        div {
    ///            id: "click-count",
    ///            "Click count: {click_count}"
    ///        }
    ///    }
    /// }
    /// # async fn run_test() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let tester = dioxus_test::render(AComponent).build();
    /// tester.query("#click-count").expect(inner_html(contains_substring("Click count: 0"))).await?;
    /// tester.query("button").click().await?;
    /// tester.query("#click-count").expect(inner_html(contains_substring("Click count: 1"))).await?;
    /// # Ok(())
    /// # }
    /// # tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap().block_on(run_test()).unwrap();
    /// ```
    ///
    /// Panics if the query contains a syntactically invalid CSS selector.
    pub fn query(&self, query: impl TryIntoSelector) -> ElementCondition<'_> {
        let document = self.document.borrow_mut();
        let error = query.to_error_builder();
        let rendered_query = query.to_string();
        let selector = query
            .try_into_selector(&document)
            .expect("Invalid CSS selector");
        ElementCondition::new(self, rendered_query, selector, error)
    }

    /// Returns a representation of elements in the DOM satisfying the given query.
    ///
    /// The query can be anything which dereferences to a `str`, including `&str` and `String`. This
    /// method then interprets it as a CSS selector. Alternatively, one can select by testid with
    /// [by_testid].
    ///
    /// The test can immediately resolve the set of elements in order to assert on or interact with
    /// them, or it can make an assertion and drive the event loop until that assertion to be true.
    /// See [AllElementsCondition] for more.
    ///
    /// Panics if the query contains a syntactically invalid CSS selector.
    pub fn query_all(&self, query: impl TryIntoSelector) -> AllElementsCondition<'_> {
        let document = self.document.borrow_mut();
        let rendered_query = query.to_string();
        let selector = query
            .try_into_selector(&document)
            .expect("Invalid CSS selector");
        AllElementsCondition::new(self, rendered_query, selector)
    }

    pub(crate) fn build_resolved_element(&self, id: usize) -> ResolvedElement {
        ResolvedElement {
            document: self.document.clone(),
            node_id: NodeId::Node(id),
        }
    }
}

/// A value which can be turned into a CSS selector to query the DOM.
///
/// This is implemented for all types which dereference to `str`, including `&str` and `String`.
///
/// One can also select by [testid](https://testing-library.com/docs/queries/bytestid/) using the
/// function [by_testid].
pub trait TryIntoSelector: std::fmt::Display {
    fn try_into_selector(self, document: &DioxusDocument) -> Result<SelectorList, TesterError>;

    fn to_error_builder(&self) -> Rc<ErrorBuilder>;
}

impl<T: AsRef<str> + std::fmt::Display> TryIntoSelector for T {
    fn try_into_selector(self, document: &DioxusDocument) -> Result<SelectorList, TesterError> {
        document
            .inner()
            .try_parse_selector_list(self.as_ref())
            .map_err(|_| {
                TesterError::InvalidCssSelector(format!("Invalid CSS selector '{}'", self.as_ref()))
            })
    }

    fn to_error_builder(&self) -> Rc<ErrorBuilder> {
        let selector: String = self.as_ref().into();
        Rc::new(move |dom| TesterError::NoSuchElementWithCssSelector(selector.clone(), dom))
    }
}

struct QueryByTestId(String);

impl TryIntoSelector for QueryByTestId {
    fn try_into_selector(self, document: &DioxusDocument) -> Result<SelectorList, TesterError> {
        Ok(document
            .inner()
            .try_parse_selector_list(&format!(r#"[data-testid="{}"]"#, self.0))
            .expect("Selector with testid should always parse"))
    }

    fn to_error_builder(&self) -> Rc<ErrorBuilder> {
        let testid = self.0.clone();
        Rc::new(move |dom| TesterError::NoSuchElementWithTestId(testid.clone(), dom))
    }
}

impl std::fmt::Display for QueryByTestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, r#"[data-testid="{}"]"#, self.0)
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
pub fn by_testid(testid: impl AsRef<str>) -> impl TryIntoSelector {
    QueryByTestId(testid.as_ref().to_string())
}

#[cfg(test)]
mod tests {
    use crate::{Result, by_testid, matchers::inner_html, render};
    use dioxus::prelude::*;
    use indoc::indoc;
    use test_that::prelude::*;

    #[tokio::test]
    async fn query_all_allows_matching_multiple_elements() -> Result<()> {
        #[component]
        fn MyComponent() -> Element {
            rsx! {
                div {
                     class: "some-class",
                }
                div {
                     class: "some-class",
                }
            }
        }
        let tester = render(MyComponent).build();

        tester
            .query_all(".some-class")
            .expect(len(eq(2)))
            .immediately()
    }

    #[tokio::test]
    async fn assertion_failure_message_includes_query_actual_value_description_and_explanation()
    -> TestResult<()> {
        #[component]
        fn MyComponent() -> Element {
            rsx! {
                div {
                     "data-testid": "the-label",
                     "Actual value"
                }
            }
        }
        let tester = render(MyComponent).build();

        let result = tester
            .query(by_testid("the-label"))
            .expect(inner_html(eq("Expected value")))
            .immediately();

        verify_that!(
            result,
            err(displays_as(eq(indoc!(
                r#"
                Element: [data-testid="the-label"]
                Expected: has inner HTML which
                  is equal to "Expected value"
                But was:
                  <div data-testid="the-label">
                    Actual value
                  </div>
                which has inner HTML which
                  isn't equal to "Expected value""#
            ))))
        )
    }

    #[tokio::test]
    async fn assertion_failure_message_includes_all_matched_elements_for_query_all()
    -> TestResult<()> {
        #[component]
        fn MyComponent() -> Element {
            rsx! {
                div {
                     "data-testid": "the-label",
                     "Actual value 1"
                }
            }
        }
        let tester = render(MyComponent).build();

        let result = tester
            .query_all(by_testid("the-label"))
            .expect(empty())
            .immediately();

        verify_that!(
            result,
            err(displays_as(eq(indoc!(
                r#"
                Element: [data-testid="the-label"]
                Expected: is empty
                But was:
                [
                  <div data-testid="the-label">
                    Actual value 1
                  </div>
                ]
                which isn't empty"#
            ))))
        )
    }

    #[tokio::test]
    async fn document_allows_multiple_unresolved_queries_in_parallel() {
        #[component]
        fn MyComponent() -> Element {
            let mut text = use_signal(|| "Click me!");
            let mut label = use_signal(|| "Not clicked yet");
            rsx! {
                div {
                     "data-testid": "the-label",
                     {label}
                }
                button {
                     class: "test-button",
                     onclick: move |_| {
                         *text.write() = "Clicked";
                         *label.write() = "Now clicked";
                     },
                     {text}
                }
            }
        }

        let tester = render(MyComponent).build();
        let test_button = tester.query(".test-button");
        let label = tester.query(by_testid("the-label"));
        tester.query(".test-button").click().await.unwrap();
        test_button.expect(inner_html(eq("Clicked"))).await.unwrap();
        label
            .expect(inner_html(eq("Now clicked")))
            .immediately()
            .unwrap();
    }

    #[test]
    fn assertion_failure_message_includes_dom_when_no_element_matches_css_query() -> TestResult<()>
    {
        #[component]
        fn MyComponent() -> Element {
            rsx! {
                div {
                    class: "arbitrary-class"
                }
            }
        }
        let tester = render(MyComponent).build();

        let result = tester
            .query(".different-class")
            .expect(anything())
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(indoc!(
                r#"
                No such element with CSS selector `.different-class`
                DOM is:
                <html>
                  <head />
                  <body>
                    <main id="main">
                      <div class="arbitrary-class" />
                    </main>
                  </body>
                </html>
                "#
            ))))
        )
    }

    #[test]
    fn assertion_failure_message_includes_dom_when_no_element_has_testid() -> TestResult<()> {
        #[component]
        fn MyComponent() -> Element {
            rsx! {
                div {
                    "data-testid": "Arbitrary testid"
                }
            }
        }
        let tester = render(MyComponent).build();

        let result = tester
            .query(by_testid("Different testid"))
            .expect(anything())
            .immediately();

        verify_that!(
            result,
            err(displays_as(contains_substring(indoc!(
                r#"
                No such element with test ID `Different testid`
                DOM is:
                <html>
                  <head />
                  <body>
                    <main id="main">
                      <div data-testid="Arbitrary testid" />
                    </main>
                  </body>
                </html>
                "#
            ))))
        )
    }

    #[tokio::test]
    async fn assertion_failure_message_includes_dom_when_element_was_awaited() -> TestResult<()> {
        #[component]
        fn MyComponent() -> Element {
            rsx! {
                div {
                    "data-testid": "Arbitrary testid"
                }
            }
        }
        let tester = render(MyComponent).build();

        let result = tester
            .query(by_testid("Different testid"))
            .expect(anything())
            .await;

        verify_that!(
            result,
            err(displays_as(contains_substring(indoc!(
                r#"
                No such element with test ID `Different testid`
                DOM is:
                <html>
                  <head />
                  <body>
                    <main id="main">
                      <div data-testid="Arbitrary testid" />
                    </main>
                  </body>
                </html>
                "#
            ))))
        )
    }
}
