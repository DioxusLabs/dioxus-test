pub type Result<T> = std::result::Result<T, TesterError>;

pub(crate) type ErrorBuilder = dyn Fn(String) -> TesterError;

/// An error during resolution of a DOM element or making an assertion.
///
/// This normally indicates that the test should fail.
#[derive(Clone)]
pub enum TesterError {
    /// The given CSS selector had invalid syntax.
    InvalidCssSelector(String),

    /// No element with the test ID, as given by the HTML attribute `data-testid`, was found in the
    /// DOM.
    NoSuchElementWithTestId(String, String),

    /// No element matching the given CSS selector was found in the DOM.
    NoSuchElementWithCssSelector(String, String),

    /// An assertion on a test element failed
    AssertionFailure(String),
}

impl std::fmt::Display for TesterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TesterError::InvalidCssSelector(selector) => {
                write!(f, "Invalid CSS selector {selector}")
            }
            TesterError::NoSuchElementWithTestId(id, dom) => {
                write!(f, "No such element with test ID `{id}`\nDOM is:\n{dom}")
            }
            TesterError::NoSuchElementWithCssSelector(selector, dom) => {
                write!(
                    f,
                    "No such element with CSS selector `{selector}`\nDOM is:\n{dom}"
                )
            }
            TesterError::AssertionFailure(description) => {
                write!(f, "Failed assertion: {description}")
            }
        }
    }
}

impl std::fmt::Debug for TesterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self, f)
    }
}

impl std::error::Error for TesterError {}
