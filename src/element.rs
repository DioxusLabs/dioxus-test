use crate::TesterError;
use blitz_dom::{BaseDocument, Document as _, Node, Point};
use blitz_traits::events::BlitzFocusEvent;
use dioxus_core::{ElementId, Event};
use dioxus_html::{
    Modifiers, PlatformEventData,
    geometry::{Coordinates, euclid::Point2D},
};
use dioxus_native_dom::{DioxusDocument, synthetic_click_event, synthetic_form_event};
use std::{
    cell::{RefCell, RefMut},
    ops::Deref,
    rc::Rc,
};

/// A reference to DOM node managed by a [crate::DocumentTester].
///
/// This provides facilities for interacting with the node, querying its layout properties, and
/// obtaining its content.
pub struct ResolvedElement {
    pub(crate) document: Rc<RefCell<DioxusDocument>>,
    pub(crate) node_id: NodeId,
}

impl ResolvedElement {
    /// Dispatches a `click` event on this element.
    ///
    /// The exact location of the click is unspecified.
    ///
    /// If the element has an `onclick` handler, it will be invoked once
    /// [crate::DocumentTester::pump] is called.
    pub fn click(&self) -> crate::Result<()> {
        let guard = self.document.borrow();
        let event = Event::new(
            Rc::new(PlatformEventData::new(synthetic_click_event(
                self.node_id.resolve(&guard.inner()),
                Modifiers::empty(),
            ))),
            true,
        );
        drop(guard);
        self.send_event("click", event)
    }

    /// Dispatches an `input` event on this element with the given `text`.
    ///
    /// If this element accepts keyboard input (e.g., if it is an `<input>` or `<textarea>`
    /// element), then the text input will be processed by the `oninput` event handler.
    ///
    /// This does not respect the keyboard focus.
    pub(crate) fn input(&self, text: impl Into<String>) -> Result<(), TesterError> {
        let guard = self.document.borrow();
        let event = Event::new(
            Rc::new(PlatformEventData::new(synthetic_form_event(text, vec![]))),
            true,
        );
        drop(guard);
        self.send_event("input", event)
    }

    /// Sets the keyboard focus to this element.
    ///
    /// This means that keyboard focus events triggered through
    /// [DocumentTester::key_down][crate::DocumentTester::key_down] and
    /// [DocumentTester::key_up][crate::DocumentTester::key_up] will be routed through this element.
    pub fn focus(&self) -> crate::Result<()> {
        change_focus(self.document.borrow_mut(), self.node_id);
        Ok(())
    }

    /// Sends an event with the given `name` to this element.
    ///
    /// The event is registered with the Dioxus runtime. A subsequent call to
    /// [crate::DocumentTester::pump] causes the event handler to be invoked, if one is present.
    ///
    /// If no event handler is registered corresponding to the event `name`, then this method has no
    /// effect.
    ///
    /// This operates directly on the element, so that is is guaranteed to receive the event. This
    /// might not reflect how the element would respond in reality. For example, a click at the
    /// coordinates of a button which is behind a frost element will not reach the button. But this
    /// method behaves as though it would.
    ///
    /// The `event` parameter must contain a [PlatformEventData] with a payload corresponding to the
    /// specific event type. This method panics if the event payload has the wrong type.
    pub fn send_event(&self, name: &str, event: Event<PlatformEventData>) -> crate::Result<()> {
        let propagates = event.propagates();
        let Some(element_id) = self.get_element_id() else {
            return Err(TesterError::InteractionWithNonInteractiveElement(
                name.to_string(),
                self.outer_html(),
            ));
        };
        let mut document = self.document.borrow_mut();
        document
            .vdom
            .runtime()
            .handle_event(name, Event::new(event.data, propagates), element_id);
        // Process any effects which were triggered but not executed immediately during rendering,
        // and rerender the vdom to reflect any state changes they make.
        while document.poll(None) {}
        Ok(())
    }

    /// Returns a `String` consisting of the HTML of this element and all of its children.
    pub fn outer_html(&self) -> String {
        let guard = self.document.borrow();
        self.node_id.resolve(&guard.inner()).outer_html_pretty()
    }

    /// Returns a `String` consisting of the HTML of this element's children, not including this
    /// element itself.
    pub fn inner_html(&self) -> String {
        let guard = self.document.borrow();
        let inner_html_parts: Vec<_> = self
            .node_id
            .resolve(&guard.inner())
            .children
            .iter()
            .filter_map(|child_id| {
                guard
                    .inner()
                    .get_node(*child_id)
                    .map(|child| child.outer_html())
            })
            .collect();
        inner_html_parts.join("")
    }

    /// Returns the calculated [Coordinates] of the centre of this element.
    pub fn center(&self) -> Coordinates {
        let upper_left = self.upper_left();
        let lower_right = self.lower_right();
        Coordinates::new(
            upper_left.screen().lerp(lower_right.screen(), 0.5),
            upper_left.client().lerp(lower_right.client(), 0.5),
            upper_left.element().lerp(lower_right.element(), 0.5),
            upper_left.page().lerp(lower_right.page(), 0.5),
        )
    }

    /// Returns the calculated [Coordinates] of the upper-left corner of this element.
    pub fn upper_left(&self) -> Coordinates {
        let guard = self.document.borrow();
        let document = guard.inner();
        let node = self.node_id.resolve(&document);
        let upper_left = Point {
            x: node.final_layout.location.x,
            y: node.final_layout.location.y,
        };
        Coordinates::new(
            Self::to_point2d(upper_left),
            Self::to_point2d(upper_left),
            Self::to_point2d(upper_left),
            Self::to_point2d(upper_left),
        )
    }

    /// Returns the calculated [Coordinates] of the upper-right corner of this element.
    pub fn upper_right(&self) -> Coordinates {
        let guard = self.document.borrow();
        let document = guard.inner();
        let node = self.node_id.resolve(&document);
        let mut upper_right = Point {
            x: node.final_layout.location.x,
            y: node.final_layout.location.y,
        };
        upper_right.x += node.final_layout.content_box_width();
        Coordinates::new(
            Self::to_point2d(upper_right),
            Self::to_point2d(upper_right),
            Self::to_point2d(upper_right),
            Self::to_point2d(upper_right),
        )
    }

    /// Returns the calculated [Coordinates] of the lower-left corner of this element.
    pub fn lower_left(&self) -> Coordinates {
        let guard = self.document.borrow();
        let document = guard.inner();
        let node = self.node_id.resolve(&document);
        let mut lower_left = Point {
            x: node.final_layout.location.x,
            y: node.final_layout.location.y,
        };
        lower_left.y += node.final_layout.content_box_height();
        Coordinates::new(
            Self::to_point2d(lower_left),
            Self::to_point2d(lower_left),
            Self::to_point2d(lower_left),
            Self::to_point2d(lower_left),
        )
    }

    /// Returns the calculated [Coordinates] of the lower-right corner of this element.
    pub fn lower_right(&self) -> Coordinates {
        let guard = self.document.borrow();
        let document = guard.inner();
        let node = self.node_id.resolve(&document);
        let mut lower_right = Point {
            x: node.final_layout.location.x,
            y: node.final_layout.location.y,
        };
        lower_right.x += node.final_layout.content_box_width();
        lower_right.y += node.final_layout.content_box_height();
        Coordinates::new(
            Self::to_point2d(lower_right),
            Self::to_point2d(lower_right),
            Self::to_point2d(lower_right),
            Self::to_point2d(lower_right),
        )
    }

    fn to_point2d<Space>(point: Point<f32>) -> Point2D<f64, Space> {
        Point2D::new(point.x as f64, point.y as f64)
    }

    /// Returns the calculated size of this element as a tuple (width, height) in screen pixels.
    pub fn size(&self) -> (f32, f32) {
        let guard = self.document.borrow();
        let document = guard.inner();
        let node = self.node_id.resolve(&document);
        let height = node.final_layout.content_box_height();
        let width = node.final_layout.content_box_width();
        (width, height)
    }

    fn get_element_id(&self) -> Option<ElementId> {
        let guard = self.document.borrow();
        get_element_id(&guard.inner(), self.node_id)
    }

    pub(crate) fn attribute(&self, arg: &str) -> Option<String> {
        let guard = self.document.borrow();
        self.node_id
            .resolve(&guard.inner())
            .element_data()?
            .attrs
            .iter()
            .find(|attr| *attr.name.local == *arg)
            .map(|attr| attr.value.clone())
    }

    pub(crate) fn has_focus(&self) -> bool {
        let document = self.document.borrow();
        let base_document = document.inner();
        let focus_node_id = base_document.get_focussed_node_id();
        let this_node_id = self.node_id.into_raw_id(&base_document);
        focus_node_id == Some(this_node_id)
    }

    pub(crate) fn focus_node_html(&self) -> Option<String> {
        let document = self.document.borrow();
        let base_document = document.inner();
        let focus_node_id = base_document.get_focussed_node_id()?;
        let focus_node = base_document.get_node(focus_node_id)?;
        Some(focus_node.outer_html_pretty())
    }
}

impl std::fmt::Debug for ResolvedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedElement")
            .field("node_id", &self.node_id)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum NodeId {
    Root,
    Node(usize),
}

impl NodeId {
    pub(crate) fn into_raw_id<T: Deref<Target = BaseDocument>>(self, document: &T) -> usize {
        match self {
            NodeId::Root => document.root_node().id,
            NodeId::Node(node_id) => node_id,
        }
    }

    fn resolve<'doc, T: Deref<Target = BaseDocument> + 'doc>(
        self,
        document: &'doc T,
    ) -> &'doc Node {
        match self {
            NodeId::Root => document.root_element(),
            NodeId::Node(node_id) => document
                .get_node(node_id)
                .expect("Element must be attached"),
        }
    }
}

fn change_focus(mut guard: RefMut<DioxusDocument>, new_focus_node_id: NodeId) {
    let mut base_document = guard.inner_mut();
    let new_focus_id = new_focus_node_id.into_raw_id(&base_document);
    let old_focus_id = base_document.get_focussed_node_id();
    base_document.set_focus_to(new_focus_id);
    drop(base_document);
    if let Some(old_focus_id) = old_focus_id
        && let Some(element_id) = get_element_id(&guard.inner(), NodeId::Node(old_focus_id))
    {
        guard.vdom.runtime().handle_event(
            "blur",
            Event::new(
                Rc::new(PlatformEventData::new(Box::new(BlitzFocusEvent))),
                false,
            ),
            element_id,
        );
        guard.vdom.runtime().handle_event(
            "focusout",
            Event::new(
                Rc::new(PlatformEventData::new(Box::new(BlitzFocusEvent))),
                true,
            ),
            element_id,
        );
    }
    if let Some(element_id) = get_element_id(&guard.inner(), new_focus_node_id) {
        guard.vdom.runtime().handle_event(
            "focus",
            Event::new(
                Rc::new(PlatformEventData::new(Box::new(BlitzFocusEvent))),
                false,
            ),
            element_id,
        );
        guard.vdom.runtime().handle_event(
            "focusin",
            Event::new(
                Rc::new(PlatformEventData::new(Box::new(BlitzFocusEvent))),
                true,
            ),
            element_id,
        );
    }
}

fn get_element_id(guard: &impl Deref<Target = BaseDocument>, node_id: NodeId) -> Option<ElementId> {
    let element_data = node_id.resolve(guard).element_data()?;
    let attr = element_data
        .attrs
        .iter()
        .find(|attr| *attr.name.local == *"data-dioxus-id")?;
    Some(ElementId(attr.value.parse::<usize>().ok()?))
}
