use crate::element::ResolvedElement;
pub use test_that::matchers::{containers::*, *};
use test_that::{
    description::Description,
    matcher::{Describable, MatcherResult},
    prelude::Matcher,
};

/// Returns a [Matcher] which matches an element whose inner HTML is matched by the [Matcher]
/// `inner`.
pub fn inner_html(inner: impl Matcher<String>) -> impl Matcher<ResolvedElement> {
    struct InnerHtmlMatcher<InnerMatcher>(InnerMatcher);

    impl<InnerMatcher: Matcher<String>> Matcher<ResolvedElement> for InnerHtmlMatcher<InnerMatcher> {
        fn matches(&self, actual: &ResolvedElement) -> MatcherResult {
            let inner_html = actual.inner_html();
            self.0.matches(&inner_html)
        }
    }

    impl<InnerMatcher: Describable> Describable for InnerHtmlMatcher<InnerMatcher> {
        fn describe(&self, matcher_result: MatcherResult) -> test_that::description::Description {
            Description::new()
                .text("Has inner HTML matching")
                .nested(self.0.describe(matcher_result))
        }
    }

    InnerHtmlMatcher(inner)
}

/// Returns a [Matcher] which matches an element whose element `name` is matched by the [Matcher]
/// `inner`.
///
/// Because the attribute might not exist on the element, the inner matcher must match an
/// `Option<String>`. When asserting that the attribute exists, use `some()`:
///
/// ```
/// # use dioxus::prelude::*;
/// # use dioxus_test::{by_testid, matchers::{attribute, eq, some}, render};
/// #[component]
/// fn TestComponent() -> Element {
///     rsx! {
///         div {
///             "data-testid": "item",
///             "my-attribute": "A value",
///         }
///     }
/// }
/// let tester = render(TestComponent).build();
/// tester.query(by_testid("item"))
///     .expect(attribute("my-attribute", some(eq("A value"))))
///     .immediately()
/// # .unwrap();
/// ```
///
/// When assertings its absence, use `none()`:
///
/// ```
/// # use dioxus::prelude::*;
/// # use dioxus_test::{by_testid, matchers::{attribute, none}, render};
/// #[component]
/// fn TestComponent() -> Element {
///     rsx! {
///         div {
///             "data-testid": "item",
///         }
///     }
/// }
/// let tester = render(TestComponent).build();
/// tester.query(by_testid("item"))
///     .expect(attribute("my-attribute", none()))
///     .immediately()
/// # .unwrap();
/// ```
pub fn attribute(
    name: impl AsRef<str>,
    inner: impl Matcher<Option<String>>,
) -> impl Matcher<ResolvedElement> {
    struct AttributeMatcher<InnerMatcher>(String, InnerMatcher);

    impl<InnerMatcher: Matcher<Option<String>>> Matcher<ResolvedElement>
        for AttributeMatcher<InnerMatcher>
    {
        fn matches(&self, actual: &ResolvedElement) -> MatcherResult {
            let attribute_content = actual.attribute(&self.0);
            self.1.matches(&attribute_content)
        }
    }

    impl<InnerMatcher: Describable> Describable for AttributeMatcher<InnerMatcher> {
        fn describe(&self, matcher_result: MatcherResult) -> test_that::description::Description {
            Description::new()
                .text("Has inner HTML matching")
                .nested(self.1.describe(matcher_result))
        }
    }

    AttributeMatcher(name.as_ref().to_string(), inner)
}

#[cfg(test)]
mod tests {
    use crate::{Result, by_testid, matchers::attribute, render};
    use dioxus::prelude::*;
    use test_that::prelude::*;

    #[tokio::test]
    async fn attribute_matches_when_asserting_async() -> Result<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div {
                    "data-testid": "item",
                    "my-attribute": "Arbitrary value",
                }
            }
        }
        let tester = render(TestComponent).build();

        tester
            .query(by_testid("item"))
            .expect(attribute("my-attribute", some(eq("Arbitrary value"))))
            .await
    }

    #[test]
    fn attribute_does_not_match_when_attribute_has_wrong_value() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div {
                    "data-testid": "item",
                    "my-attribute": "Arbitrary value",
                }
            }
        }
        let tester = render(TestComponent).build();

        let result = tester
            .query(by_testid("item"))
            .expect(attribute("my-attribute", some(eq("A different value"))))
            .immediately();

        verify_that!(result, err(anything()))
    }

    #[test]
    fn attribute_does_not_match_when_attribute_is_missing() -> TestResult<()> {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div {
                    "data-testid": "item",
                }
            }
        }
        let tester = render(TestComponent).build();

        let result = tester
            .query(by_testid("item"))
            .expect(attribute("my-attribute", some(eq("A different value"))))
            .immediately();

        verify_that!(result, err(anything()))
    }

    #[test]
    fn attribute_does_not_match_when_attribute_is_present_but_asserting_absence() -> TestResult<()>
    {
        #[component]
        fn TestComponent() -> Element {
            rsx! {
                div {
                    "data-testid": "item",
                    "my-attribute": "Arbitrary value",
                }
            }
        }
        let tester = render(TestComponent).build();

        let result = tester
            .query(by_testid("item"))
            .expect(attribute("my-attribute", none()))
            .immediately();

        verify_that!(result, err(anything()))
    }
}
