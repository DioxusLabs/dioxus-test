use crate::{DocumentTester, TesterError, condition::ElementFraction};
use blitz_dom::{BaseDocument, Document as _, Node, Point};
use blitz_traits::events::{
    BlitzFocusEvent, BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons,
    PointerCoords,
};
use dioxus::core::{ElementId, Event};
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
    pub(crate) now: f64,
    /// The coordinates at which a pointer action will take place on this element. If not present,
    /// then these are taken to be the centre of the element.
    pub(crate) action_coords: Option<Coordinates>,
}

impl ResolvedElement {
    pub(crate) fn new(
        document: Rc<RefCell<DioxusDocument>>,
        node_id: NodeId,
        now: f64,
        action_position: Option<Point2D<f32, ElementFraction>>,
    ) -> Self {
        let action_coords = action_position
            .map(|position| Self::to_coordinates(document.clone(), node_id, position));
        Self {
            document,
            node_id,
            now,
            action_coords,
        }
    }

    /// Dispatches a `click` event on this element.
    ///
    /// The exact location of the click is unspecified.
    ///
    /// The framework then invokes the `onclick` handler on the next opportunity (see
    /// [`DocumentTester::crate`](crate::DocumentTester::pump)), if present. The other events
    /// associated with dispatching a click event, including `pointerdown` and `pointerup` are _not_
    /// dispatched. To dispatch all such events, use [`tap()`][ResolvedElement::tap].
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

    /// Dispatches the events associated with tapping the centre of the element.
    ///
    /// This results in the runtime dispatching the following events:
    ///
    /// - `pointerover`
    /// - `pointerenter`
    /// - `pointerdown`
    /// - `pointerup`
    /// - `click`
    ///
    /// These are invoked at the next opportunity; see
    /// [`DocumentTester::crate`](crate::DocumentTester::pump).
    pub fn tap(&self) -> crate::Result<()> {
        let event = self.create_pointer_event(
            BlitzPointerId::finger(0),
            MouseEventButtons::from(MouseEventButton::Main),
        );
        let mut document = self.document.borrow_mut();
        document.handle_ui_event(blitz_traits::events::UiEvent::PointerDown(event.clone()));
        document.handle_ui_event(blitz_traits::events::UiEvent::PointerUp(event));
        DocumentTester::resolve_effects_and_styles(document, self.now);
        Ok(())
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

    /// Dispatches an event to simulate the pointer moving over the centre of this element.
    ///
    /// This causes the `pointerenter` event to be dispatched on the element. The runtime will
    /// then invoke the `onpointerenter` event handler on the next opportunity, if present.
    pub fn hover(&self) -> crate::Result<()> {
        let event = self.create_pointer_event(BlitzPointerId::mouse(), MouseEventButtons::empty());
        let mut document = self.document.borrow_mut();
        document.handle_ui_event(blitz_traits::events::UiEvent::PointerMove(event));
        DocumentTester::resolve_effects_and_styles(document, self.now);
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
        let document = self.document.borrow_mut();
        document
            .vdom
            .runtime()
            .handle_event(name, Event::new(event.data, propagates), element_id);
        DocumentTester::resolve_effects_and_styles(document, self.now);
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
        Self::to_coordinates(self.document.clone(), self.node_id, Point2D::new(0.5, 0.5))
    }

    /// Returns the calculated [Coordinates] of the upper-left corner of this element.
    pub fn upper_left(&self) -> Coordinates {
        Self::to_coordinates(self.document.clone(), self.node_id, Point2D::new(0.0, 0.0))
    }

    /// Returns the calculated [Coordinates] of the upper-right corner of this element.
    pub fn upper_right(&self) -> Coordinates {
        Self::to_coordinates(self.document.clone(), self.node_id, Point2D::new(1.0, 0.0))
    }

    /// Returns the calculated [Coordinates] of the lower-left corner of this element.
    pub fn lower_left(&self) -> Coordinates {
        Self::to_coordinates(self.document.clone(), self.node_id, Point2D::new(0.0, 1.0))
    }

    /// Returns the calculated [Coordinates] of the lower-right corner of this element.
    pub fn lower_right(&self) -> Coordinates {
        Self::to_coordinates(self.document.clone(), self.node_id, Point2D::new(1.0, 1.0))
    }

    fn to_coordinates(
        document: Rc<RefCell<DioxusDocument>>,
        node_id: NodeId,
        position: Point2D<f32, ElementFraction>,
    ) -> Coordinates {
        let guard = document.borrow();
        let document = guard.inner();
        let node = node_id.resolve(&document);
        let resolved_point = node.absolute_position(
            position.x * node.final_layout().content_box_width(),
            position.y * node.final_layout().content_box_height(),
        );
        Coordinates::new(
            Self::to_point2d(resolved_point),
            Self::to_point2d(resolved_point),
            Self::to_point2d(resolved_point),
            Self::to_point2d(resolved_point),
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
        let height = node.final_layout().content_box_height();
        let width = node.final_layout().content_box_width();
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

    fn create_pointer_event(
        &self,
        id: BlitzPointerId,
        buttons: MouseEventButtons,
    ) -> BlitzPointerEvent {
        let coords = self.action_coords();

        BlitzPointerEvent {
            id,
            is_primary: true,
            coords: PointerCoords {
                page_x: coords.page().x as f32,
                page_y: coords.page().y as f32,
                screen_x: coords.screen().x as f32,
                screen_y: coords.screen().y as f32,
                client_x: coords.client().x as f32,
                client_y: coords.client().y as f32,
            },
            mods: Modifiers::empty(),
            button: MouseEventButton::Main,
            buttons,
            details: Default::default(),
            element: blitz_traits::events::Point {
                x: coords.element().x as f32,
                y: coords.element().y as f32,
            },
            active_pointers: Default::default(),
        }
    }

    fn action_coords(&self) -> Coordinates {
        if let Some(action_coords) = &self.action_coords {
            // Coordinates does not implement Clone, so we clone by hand.
            Coordinates::new(
                action_coords.screen(),
                action_coords.client(),
                action_coords.element(),
                action_coords.page(),
            )
        } else {
            self.center()
        }
    }

    /// Specifies that any pointer action on this element should occur at the given [Coordinates].
    pub fn at(self, coordinates: Coordinates) -> Self {
        Self {
            action_coords: Some(coordinates),
            ..self
        }
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
    Node(blitz_dom::NodeId),
}

impl NodeId {
    pub(crate) fn into_raw_id<T: Deref<Target = BaseDocument>>(
        self,
        document: &T,
    ) -> blitz_dom::NodeId {
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
    Some(ElementId::from_raw(attr.value.parse::<usize>().ok()?))
}

#[cfg(test)]
mod tests {
    use crate::{
        Result, by_testid,
        matchers::{eq, inner_html},
        render,
    };
    use dioxus::prelude::*;

    #[tokio::test]
    async fn tap_invokes_each_event_handler() -> Result<()> {
        #[component]
        fn MyComponent() -> Element {
            let mut events = use_signal(Vec::<String>::new);
            rsx! {
                div {
                    "data-testid": "element",
                    onpointerover: move |_| events.write().push("pointerover".into()),
                    onpointerenter: move |_| events.write().push("pointerenter".into()),
                    onpointerdown: move |_| events.write().push("pointerdown".into()),
                    onpointerup: move |_| events.write().push("pointerup".into()),
                    onclick: move |_| events.write().push("click".into()),
                    {events.read().join(",")}
                }
            }
        }
        let tester = render(MyComponent);

        tester.query(by_testid("element")).tap().await?;

        tester
            .query(by_testid("element"))
            .expect(inner_html(eq(
                "pointerover,pointerenter,pointerdown,pointerup,click",
            )))
            .await
    }

    #[tokio::test]
    async fn tap_hits_element_offset_from_the_document_origin() -> Result<()> {
        // Regression test: pointer actions must hit-test at the element's absolute page
        // position (`Node::absolute_position`), not at its containing-block-relative
        // `final_layout().location`. The spacer pushes the target away from the document
        // origin (body margin), so the two coordinates differ; the 1px-tall target makes
        // the relative coordinates miss it entirely, so no handler fires.
        #[component]
        fn MyComponent() -> Element {
            let mut tapped = use_signal(|| false);
            rsx! {
                div { style: "height: 20px; width: 20px;" }
                div {
                    "data-testid": "target",
                    style: "height: 1px; width: 100px;",
                    onpointerdown: move |_| tapped.set(true),
                    {if tapped() { "tapped" } else { "" }}
                }
            }
        }
        let tester = render(MyComponent);

        tester.query(by_testid("target")).tap().await?;

        tester
            .query(by_testid("target"))
            .expect(inner_html(eq("tapped")))
            .await
    }

    #[tokio::test]
    async fn tap_at_dispatches_pointer_event_at_the_given_fraction_of_the_element() -> Result<()> {
        #[component]
        fn MyComponent() -> Element {
            let mut x = use_signal(|| 0.0f64);
            rsx! {
                div {
                    "data-testid": "target",
                    style: "width: 200px; height: 20px;",
                    onpointerdown: move |evt| x.set(evt.client_coordinates().x),
                }
                div { "data-testid": "result", {format!("{:.0}", x())} }
            }
        }
        let tester = render(MyComponent);
        let expected_x = {
            let target = tester.query(by_testid("target")).immediately()?;
            target.upper_left().client().x + 0.9 * target.size().0 as f64
        };

        tester.query(by_testid("target")).at(0.9, 0.5).tap().await?;

        tester
            .query(by_testid("result"))
            .expect(inner_html(eq(format!("{:.0}", expected_x))))
            .await
    }
}
