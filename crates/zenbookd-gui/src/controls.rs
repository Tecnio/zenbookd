use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, Rgba, SharedString, Stateful, Window,
    div, prelude::*, px, relative,
};

use crate::{readout::Direction, theme};

const CONTROL_HEIGHT: f32 = 26.0;
const STEP_BUTTON: f32 = 26.0;
const STEPPER_VALUE_WIDTH: f32 = 72.0;
const SWITCH_WIDTH: f32 = 30.0;
const SWITCH_HEIGHT: f32 = 16.0;
const SWITCH_KNOB: f32 = 10.0;
const METER_HEIGHT: f32 = 6.0;
const MARKER_WIDTH: f32 = 2.0;
const MARKER_OVERHANG: f32 = 5.0;
const ROW_HEIGHT: f32 = 34.0;
const DISABLED_OPACITY: f32 = 0.4;

pub type StepHandler = Rc<dyn Fn(Direction, bool, &mut Window, &mut App)>;

pub type ClickHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Quiet,
}

fn clickable(element: Stateful<Div>, enabled: bool, on_click: ClickHandler) -> Stateful<Div> {
    if enabled {
        element
            .cursor_pointer()
            .on_click(move |_: &ClickEvent, window, cx| on_click(window, cx))
    } else {
        element.opacity(DISABLED_OPACITY)
    }
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    enabled: bool,
    on_click: ClickHandler,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .h(px(CONTROL_HEIGHT))
        .px(px(10.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(theme::radius_control())
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(label.into());

    let styled = match kind {
        ButtonKind::Primary => base
            .bg(theme::accent())
            .text_color(theme::accent_ink())
            .when(enabled, |this| {
                this.hover(|style| style.bg(theme::accent_hover()))
            }),

        ButtonKind::Secondary => base
            .bg(theme::raised())
            .border_1()
            .border_color(theme::rule_strong())
            .text_color(theme::text())
            .when(enabled, |this| {
                this.hover(|style| style.bg(theme::raised_hover()))
            }),

        ButtonKind::Quiet => base.text_color(theme::text_muted()).when(enabled, |this| {
            this.hover(|style| style.bg(theme::raised()).text_color(theme::text()))
        }),
    };

    clickable(styled, enabled, on_click)
}

fn step_button(
    id: SharedString,
    glyph: &'static str,
    enabled: bool,
    direction: Direction,
    on_step: StepHandler,
) -> Stateful<Div> {
    let element = div()
        .id(ElementId::Name(id))
        .size(px(STEP_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme::text_muted())
        .child(glyph);

    if enabled {
        element
            .cursor_pointer()
            .hover(|style| style.bg(theme::raised_hover()).text_color(theme::text()))
            .on_click(move |event: &ClickEvent, window, cx| {
                on_step(direction, event.modifiers().shift, window, cx)
            })
    } else {
        element.text_color(theme::rule_strong())
    }
}

pub struct Stepper {
    pub value: SharedString,
    pub pending: bool,
    pub can_down: bool,
    pub can_up: bool,
}

pub fn stepper(id: &'static str, stepper: Stepper, enabled: bool, on_step: StepHandler) -> Div {
    let value_color = if stepper.pending {
        theme::accent()
    } else {
        theme::text()
    };

    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(CONTROL_HEIGHT))
        .rounded(theme::radius_control())
        .border_1()
        .border_color(theme::rule_strong())
        .bg(theme::raised())
        .overflow_hidden()
        .when(!enabled, |this| this.opacity(DISABLED_OPACITY))
        .child(step_button(
            format!("{id}-down").into(),
            "−",
            enabled && stepper.can_down,
            Direction::Down,
            on_step.clone(),
        ))
        .child(
            div()
                .w(px(STEPPER_VALUE_WIDTH))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .border_x_1()
                .border_color(theme::rule_strong())
                .bg(theme::inset())
                .text_sm()
                .font_features(theme::tabular())
                .text_color(value_color)
                .child(stepper.value),
        )
        .child(step_button(
            format!("{id}-up").into(),
            "+",
            enabled && stepper.can_up,
            Direction::Up,
            on_step,
        ))
}

pub fn switch(id: &'static str, on: bool, enabled: bool, on_toggle: ClickHandler) -> Stateful<Div> {
    let track = div()
        .id(id)
        .flex_none()
        .w(px(SWITCH_WIDTH))
        .h(px(SWITCH_HEIGHT))
        .px(px((SWITCH_HEIGHT - SWITCH_KNOB) / 2.0 - 1.0))
        .flex()
        .items_center()
        .rounded_full()
        .border_1();

    let knob = div().size(px(SWITCH_KNOB)).rounded_full();

    let styled = if on {
        track
            .justify_end()
            .bg(theme::accent())
            .border_color(theme::accent())
            .child(knob.bg(theme::accent_ink()))
    } else {
        track
            .justify_start()
            .bg(theme::inset())
            .border_color(theme::rule_strong())
            .child(knob.bg(theme::text_faint()))
    };

    clickable(styled, enabled, on_toggle)
}

pub fn segment(
    id: impl Into<ElementId>,
    label: &'static str,
    selected: bool,
    enabled: bool,
    on_select: ClickHandler,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .h(px(CONTROL_HEIGHT - 2.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .text_sm()
        .rounded(px(3.0))
        .child(label);

    if selected {
        return base.bg(theme::raised_hover()).text_color(theme::text());
    }

    let styled = base.text_color(theme::text_muted()).when(enabled, |this| {
        this.hover(|style| style.text_color(theme::text()))
    });

    clickable(styled, enabled, on_select)
}

pub fn segments(children: impl IntoIterator<Item = Stateful<Div>>) -> Div {
    div()
        .flex()
        .flex_none()
        .p(px(1.0))
        .gap(px(1.0))
        .rounded(theme::radius_control())
        .border_1()
        .border_color(theme::rule_strong())
        .bg(theme::inset())
        .children(children)
}

pub struct Meter {
    pub fill: Option<f32>,
    pub fill_color: Rgba,
    pub marker: Option<f32>,
}

pub fn meter(meter: Meter) -> Div {
    let fill = meter.fill.map(|fraction| {
        div()
            .absolute()
            .left_0()
            .top_0()
            .h_full()
            .w(relative(fraction.clamp(0.0, 1.0)))
            .bg(meter.fill_color)
    });

    let marker = meter.marker.map(|fraction| {
        div()
            .absolute()
            .top(px(-MARKER_OVERHANG))
            .left(relative(fraction.clamp(0.0, 1.0)))
            .ml(px(-MARKER_WIDTH / 2.0))
            .w(px(MARKER_WIDTH))
            .h(px(METER_HEIGHT + 2.0 * MARKER_OVERHANG))
            .bg(theme::text())
    });

    div()
        .relative()
        .w_full()
        .h(px(METER_HEIGHT))
        .bg(theme::inset())
        .border_1()
        .border_color(theme::rule())
        .children(fill)
        .children(marker)
}

pub fn row(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(ROW_HEIGHT))
        .child(
            div()
                .text_sm()
                .text_color(theme::text())
                .child(label.into()),
        )
        .child(control)
}

pub fn row_with_note(
    label: impl Into<SharedString>,
    note: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(ROW_HEIGHT))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text())
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_faint())
                        .font_features(theme::tabular())
                        .child(note.into()),
                ),
        )
        .child(control)
}

pub fn heading(title: &'static str) -> Div {
    div()
        .pt(px(8.0))
        .pb(px(2.0))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(title)
}

pub fn section(children: impl IntoIterator<Item = AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .px(px(20.0))
        .pt(px(6.0))
        .pb(px(10.0))
        .border_b_1()
        .border_color(theme::rule())
        .children(children)
}
