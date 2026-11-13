use std::{process::Child, rc::Rc, time::Duration};

use gpui::{
    AnyElement, Context, Div, FocusHandle, FontWeight, Rgba, SharedString, Task, Window, actions,
    div, prelude::*, px,
};
use zenbookd_client::{
    ClientError, Level, charge_level, health_level,
    refresh::{self, Pattern, RefreshError},
    send_request,
};
use zenbookd_ipc::{Request, Response, ServiceStatus};

use crate::{
    controls::{
        ButtonKind, ClickHandler, Meter, StepHandler, Stepper, button, heading, meter, row,
        row_with_note, section, segment, segments, stepper, switch,
    },
    link::{Fault, Link, Notice, Severity, command_notice, status_notices},
    readout::{self, Direction, Range},
    theme,
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const SETTLE_DELAY: Duration = Duration::from_millis(400);
const REFRESH_WATCH_INTERVAL: Duration = Duration::from_millis(500);
const CYCLE_HOLD_SECONDS: u32 = 5;
const FIELD_COUNT: usize = 3;
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

const RUN_LENGTH: Range = Range {
    min: 0,
    max: 720,
    step: 5,
};

actions!(zenbookd, [Quit]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Limit,
    Period,
    Hold,
}

impl Field {
    fn index(self) -> usize {
        match self {
            Self::Limit => 0,
            Self::Period => 1,
            Self::Hold => 2,
        }
    }

    fn range(self) -> Range {
        match self {
            Self::Limit => Range {
                min: 1,
                max: 100,
                step: 5,
            },
            Self::Period => Range {
                min: 1,
                max: 365,
                step: 5,
            },
            Self::Hold => Range {
                min: 1,
                max: 720,
                step: 15,
            },
        }
    }

    fn read(self, status: &ServiceStatus) -> Option<u32> {
        match self {
            Self::Limit => Some(status.charge_limit),
            Self::Period => Some(status.full_charge_period),
            Self::Hold => status.full_charge_hold,
        }
    }

    fn write(self, status: &mut ServiceStatus, value: u32) {
        match self {
            Self::Limit => status.charge_limit = value,
            Self::Period => status.full_charge_period = value,
            Self::Hold => status.full_charge_hold = Some(value),
        }
    }

    fn request(self, value: u32) -> Request {
        match self {
            Self::Limit => Request::SetChargeLimit(value),
            Self::Period => Request::SetFullChargePeriod(value),
            Self::Hold => Request::SetFullChargeHold(value),
        }
    }

    fn display(self, value: u32) -> String {
        match self {
            Self::Limit => format!("{value}%"),
            Self::Period if value == 1 => "1 day".to_string(),
            Self::Period => format!("{value} days"),
            Self::Hold => format!("{value} min"),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Limit => "limit",
            Self::Period => "period",
            Self::Hold => "hold",
        }
    }
}

struct PixelRefresh {
    pattern: Pattern,
    minutes: u32,
    child: Option<Child>,
    stopping: bool,
    watch: Option<Task<()>>,
}

impl Drop for PixelRefresh {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            child.kill().ok();
            child.wait().ok();
        }
    }
}

pub struct Dashboard {
    focus: FocusHandle,
    link: Link,
    epoch: u64,
    drafts: [Option<u32>; FIELD_COUNT],
    settles: [Option<Task<()>>; FIELD_COUNT],
    notice: Option<Notice>,
    pixel_refresh: PixelRefresh,
    _poll: Task<()>,
}

fn level_color(level: Level) -> Rgba {
    match level {
        Level::Good => theme::good(),
        Level::Low => theme::warning(),
        Level::Critical => theme::danger(),
    }
}

fn severity_color(severity: Severity) -> Rgba {
    match severity {
        Severity::Info => theme::text_muted(),
        Severity::Warning => theme::warning(),
        Severity::Failure => theme::danger(),
    }
}

fn fetch_status() -> Result<Response, ClientError> {
    send_request(Request::GetStatus)
}

impl Dashboard {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window, cx);

        let poll = cx.spawn(async move |this, cx| {
            loop {
                let Ok(epoch) = this.update(cx, |view, _| view.epoch) else {
                    break;
                };

                let result = cx
                    .background_executor()
                    .spawn(async { fetch_status() })
                    .await;

                let applied = this.update(cx, |view, cx| view.apply_poll(epoch, result, cx));

                if applied.is_err() {
                    break;
                }

                cx.background_executor().timer(POLL_INTERVAL).await;
            }
        });

        Self {
            focus,
            link: Link::Connecting,
            epoch: 0,
            drafts: [None; FIELD_COUNT],
            settles: [None, None, None],
            notice: None,
            pixel_refresh: PixelRefresh {
                pattern: Pattern::Noise,
                minutes: 0,
                child: None,
                stopping: false,
                watch: None,
            },
            _poll: poll,
        }
    }

    fn apply_poll(
        &mut self,
        epoch: u64,
        result: Result<Response, ClientError>,
        cx: &mut Context<Self>,
    ) {
        if epoch != self.epoch {
            return;
        }

        self.link = Link::from_poll(result);
        cx.notify();
    }

    fn refetch(&mut self, cx: &mut Context<Self>) {
        let epoch = self.epoch;

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { fetch_status() })
                .await;

            this.update(cx, |view, cx| view.apply_poll(epoch, result, cx))
                .ok();
        })
        .detach();
    }

    fn command(
        &mut self,
        request: Request,
        on_success: impl FnOnce(&mut Self) + 'static,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { send_request(request) })
                .await;

            this.update(cx, |view, cx| {
                view.epoch += 1;
                view.notice = command_notice(&result);

                if view.notice.is_none() {
                    on_success(view);
                }

                view.refetch(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn status_mut(&mut self) -> Option<&mut ServiceStatus> {
        match &mut self.link {
            Link::Online(status) => Some(status),
            Link::Connecting | Link::Unreachable(_) => None,
        }
    }

    fn shown(&self, field: Field) -> Option<u32> {
        self.drafts[field.index()].or_else(|| self.link.status().and_then(|s| field.read(s)))
    }

    fn step(&mut self, field: Field, direction: Direction, fine: bool, cx: &mut Context<Self>) {
        let Some(current) = self.shown(field) else {
            return;
        };

        let next = field.range().step(current, direction, fine);

        if next == current {
            return;
        }

        self.drafts[field.index()] = Some(next);

        self.settles[field.index()] = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE_DELAY).await;

            this.update(cx, |view, cx| view.settle(field, cx)).ok();
        }));

        cx.notify();
    }

    fn settle(&mut self, field: Field, cx: &mut Context<Self>) {
        let Some(value) = self.drafts[field.index()] else {
            return;
        };

        self.settles[field.index()] = None;

        self.command(
            field.request(value),
            move |view| {
                if let Some(status) = view.status_mut() {
                    field.write(status, value);
                }
            },
            cx,
        );

        self.drafts[field.index()] = None;

        if let Some(status) = self.status_mut() {
            field.write(status, value);
        }
    }

    fn toggle_periodic(&mut self, cx: &mut Context<Self>) {
        let Some(status) = self.status_mut() else {
            return;
        };

        let enable = !status.enable_periodic_full_charge;
        status.enable_periodic_full_charge = enable;

        self.command(Request::SetPeriodicFullCharge(enable), |_| {}, cx);
        cx.notify();
    }

    fn toggle_wifi(&mut self, cx: &mut Context<Self>) {
        let Some(status) = self.status_mut() else {
            return;
        };

        let disable = !status.wifi_power_save_on_ac.unwrap_or(false);
        status.wifi_power_save_on_ac = Some(disable);

        self.command(Request::SetWifiPowerSave(disable), |_| {}, cx);
        cx.notify();
    }

    fn toggle_boost(&mut self, cx: &mut Context<Self>) {
        let Some(status) = self.link.status() else {
            return;
        };

        let start = status.boost_until.is_none();

        self.command(Request::SetBoost(start), |_| {}, cx);
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.command(
            Request::ReloadConfig,
            |view| view.notice = Some(Notice::info("Config reloaded from /etc/zenbookd")),
            cx,
        );
    }

    fn start_refresh(&mut self, cx: &mut Context<Self>) {
        let size = match refresh::detect_resolution() {
            Ok(size) => size,

            Err(err) => {
                self.notice = Some(Notice::failure("Screen size not found", &err.to_string()));
                cx.notify();

                return;
            }
        };

        let minutes = (self.pixel_refresh.minutes > 0).then_some(self.pixel_refresh.minutes);

        match refresh::spawn(
            self.pixel_refresh.pattern,
            size,
            CYCLE_HOLD_SECONDS,
            minutes,
        ) {
            Ok(child) => {
                self.notice = None;
                self.pixel_refresh.child = Some(child);
                self.pixel_refresh.stopping = false;
                self.pixel_refresh.watch = Some(self.watch_refresh(cx));
            }

            Err(RefreshError::FfplayMissing) => {
                self.notice = Some(Notice::failure(
                    "ffplay is not installed",
                    "Install ffmpeg, which provides it",
                ));
            }

            Err(err) => {
                self.notice = Some(Notice::failure(
                    "Pixel refresh could not start",
                    &err.to_string(),
                ));
            }
        }

        cx.notify();
    }

    fn watch_refresh(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_WATCH_INTERVAL).await;

                match this.update(cx, |view, cx| view.reap_refresh(cx)) {
                    Ok(false) => {}
                    Ok(true) | Err(_) => break,
                }
            }
        })
    }

    fn reap_refresh(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(child) = &mut self.pixel_refresh.child else {
            return true;
        };

        let outcome = match child.try_wait() {
            Ok(None) => return false,
            Ok(Some(status)) => refresh::finished(status).map_err(|err| err.to_string()),
            Err(err) => Err(err.to_string()),
        };

        self.pixel_refresh.child = None;

        let stopped_here = std::mem::take(&mut self.pixel_refresh.stopping);

        if let Err(err) = outcome
            && !stopped_here
        {
            self.notice = Some(Notice::failure("Pixel refresh failed", &err));
        }

        cx.notify();

        true
    }

    fn stop_refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(child) = &mut self.pixel_refresh.child {
            child.kill().ok();
            self.pixel_refresh.stopping = true;
        }

        cx.notify();
    }

    fn step_run_length(&mut self, direction: Direction, fine: bool, cx: &mut Context<Self>) {
        self.pixel_refresh.minutes = RUN_LENGTH.step(self.pixel_refresh.minutes, direction, fine);
        cx.notify();
    }

    fn select_pattern(&mut self, pattern: Pattern, cx: &mut Context<Self>) {
        self.pixel_refresh.pattern = pattern;
        cx.notify();
    }

    fn on_step(&self, field: Field, cx: &Context<Self>) -> StepHandler {
        let view = cx.weak_entity();

        Rc::new(move |direction, fine, _, cx| {
            view.update(cx, |this, cx| this.step(field, direction, fine, cx))
                .ok();
        })
    }

    fn on_click(
        &self,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> ClickHandler {
        let view = cx.weak_entity();

        Rc::new(move |_, cx| {
            view.update(cx, |this, cx| action(this, cx)).ok();
        })
    }

    fn field_stepper(&self, field: Field, enabled: bool, cx: &Context<Self>) -> Div {
        let range = field.range();
        let value = self.shown(field);

        let shown = Stepper {
            value: value
                .map_or("–".to_string(), |value| field.display(value))
                .into(),
            pending: self.drafts[field.index()].is_some(),
            can_down: value.is_some_and(|value| range.can_step(value, Direction::Down)),
            can_up: value.is_some_and(|value| range.can_step(value, Direction::Up)),
        };

        stepper(
            field.id(),
            shown,
            enabled && value.is_some(),
            self.on_step(field, cx),
        )
    }

    fn service_line(&self) -> Div {
        let (state, color) = match &self.link {
            Link::Connecting => ("connecting", theme::text_faint()),
            Link::Online(_) => ("service running", theme::good()),
            Link::Unreachable(_) => ("service unreachable", theme::danger()),
        };

        div()
            .flex()
            .items_center()
            .justify_between()
            .text_xs()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_muted())
                    .child("Battery"),
            )
            .child(div().text_color(color).child(state))
    }

    fn render_battery(&self) -> Div {
        let body: Vec<AnyElement> = match &self.link {
            Link::Online(status) => self.battery_reading(status),
            Link::Connecting => vec![
                div()
                    .text_sm()
                    .text_color(theme::text_faint())
                    .child("Reading the battery")
                    .into_any_element(),
            ],
            Link::Unreachable(fault) => fault_lines(fault),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .px(px(20.0))
            .pt(px(14.0))
            .pb(px(16.0))
            .border_b_1()
            .border_color(theme::rule())
            .child(self.service_line())
            .children(body)
    }

    fn battery_reading(&self, status: &ServiceStatus) -> Vec<AnyElement> {
        let limit = self.shown(Field::Limit).unwrap_or(status.charge_limit);

        let figure = match status.battery_charge {
            Some(charge) => div()
                .text_3xl()
                .font_weight(FontWeight::LIGHT)
                .font_features(theme::tabular())
                .text_color(level_color(charge_level(charge)))
                .child(format!("{charge}%")),

            None => div()
                .text_sm()
                .text_color(theme::text_faint())
                .child("Charge not reported"),
        };

        let health = status.battery_health.map(|health| {
            div()
                .flex()
                .gap(px(6.0))
                .child(div().text_color(theme::text_muted()).child("Health"))
                .child(
                    div()
                        .font_features(theme::tabular())
                        .text_color(level_color(health_level(health)))
                        .child(format!("{health}%")),
                )
        });

        let reading = div()
            .flex()
            .items_end()
            .justify_between()
            .child(figure)
            .child(div().pb(px(4.0)).text_sm().children(health));

        let gauge = meter(Meter {
            fill: status
                .battery_charge
                .map(|charge| charge.min(100) as f32 / 100.0),
            fill_color: status.battery_charge.map_or(theme::text_faint(), |charge| {
                level_color(charge_level(charge))
            }),
            marker: Some(limit.min(100) as f32 / 100.0),
        });

        let caption = div()
            .text_xs()
            .font_features(theme::tabular())
            .text_color(theme::text_muted())
            .child(threshold_caption(status, limit));

        vec![
            reading.into_any_element(),
            div().pt(px(4.0)).child(gauge).into_any_element(),
            caption.into_any_element(),
        ]
    }

    fn render_charge(&self, status: Option<&ServiceStatus>, cx: &Context<Self>) -> Div {
        let online = status.is_some();
        let boost_until = status.and_then(|status| status.boost_until);

        let (note, label) = match boost_until {
            Some(until) => (readout::boost_remaining(until, readout::now()), "Stop"),
            None => (
                "Back to the limit once 100% has held".to_string(),
                "Charge to 100%",
            ),
        };

        section([
            row("Charge limit", self.field_stepper(Field::Limit, online, cx)).into_any_element(),
            row_with_note(
                "Boost",
                note,
                button(
                    "boost",
                    label,
                    ButtonKind::Secondary,
                    online,
                    self.on_click(cx, Self::toggle_boost),
                ),
            )
            .into_any_element(),
        ])
    }

    fn render_full_charge(&self, status: Option<&ServiceStatus>, cx: &Context<Self>) -> Div {
        let online = status.is_some();
        let periodic = status.is_some_and(|status| status.enable_periodic_full_charge);

        let last = status.map(|status| {
            format!(
                "Last full charge {}",
                readout::since_full_charge(status.last_full_charge, readout::now())
            )
        });

        let periodic_switch = switch(
            "periodic",
            periodic,
            online,
            self.on_click(cx, Self::toggle_periodic),
        );

        let periodic_row = match last {
            Some(note) => row_with_note("Periodic full charge", note, periodic_switch),
            None => row("Periodic full charge", periodic_switch),
        };

        let held = status
            .and_then(|status| status.held_at_full.zip(status.full_charge_hold))
            .map(|(held, hold)| readout::held_at_full(held, hold));

        let hold = self.field_stepper(Field::Hold, online, cx);

        let hold_row = match held {
            Some(note) => row_with_note("Hold at 100%", note, hold),
            None => row("Hold at 100%", hold),
        };

        section([
            heading("Full charge").into_any_element(),
            periodic_row.into_any_element(),
            row(
                "Every",
                self.field_stepper(Field::Period, online && periodic, cx),
            )
            .into_any_element(),
            hold_row.into_any_element(),
        ])
    }

    fn render_wifi(&self, status: Option<&ServiceStatus>, cx: &Context<Self>) -> Div {
        let setting = status.and_then(|status| status.wifi_power_save_on_ac);

        let note = match (status, setting) {
            (Some(_), None) => "This daemon does not report the setting",
            _ => "Restored when unplugged",
        };

        section([
            heading("Wi-Fi").into_any_element(),
            row_with_note(
                "No power saving on AC",
                note,
                switch(
                    "wifi",
                    setting.unwrap_or(false),
                    setting.is_some(),
                    self.on_click(cx, Self::toggle_wifi),
                ),
            )
            .into_any_element(),
        ])
    }

    fn render_pixel_refresh(&self, cx: &Context<Self>) -> Div {
        let running = self.pixel_refresh.child.is_some();
        let pattern = self.pixel_refresh.pattern;

        let pattern_option = |id: &'static str, label: &'static str, choice: Pattern| {
            segment(
                id,
                label,
                pattern == choice,
                !running,
                self.on_click(cx, move |this, cx| this.select_pattern(choice, cx)),
            )
        };

        let view = cx.weak_entity();

        let run_length = stepper(
            "run-length",
            Stepper {
                value: readout::run_length(self.pixel_refresh.minutes).into(),
                pending: false,
                can_down: RUN_LENGTH.can_step(self.pixel_refresh.minutes, Direction::Down),
                can_up: RUN_LENGTH.can_step(self.pixel_refresh.minutes, Direction::Up),
            },
            !running,
            Rc::new(move |direction, fine, _, cx| {
                view.update(cx, |this, cx| this.step_run_length(direction, fine, cx))
                    .ok();
            }),
        );

        let (note, action) = if running {
            (
                "Running fullscreen, q or Esc ends it",
                button(
                    "refresh-stop",
                    "Stop",
                    ButtonKind::Secondary,
                    true,
                    self.on_click(cx, Self::stop_refresh),
                ),
            )
        } else {
            (
                "Fullscreen on the built-in display",
                button(
                    "refresh-start",
                    "Start",
                    ButtonKind::Primary,
                    true,
                    self.on_click(cx, Self::start_refresh),
                ),
            )
        };

        section([
            heading("Pixel refresh").into_any_element(),
            row(
                "Pattern",
                segments([
                    pattern_option("pattern-noise", "Noise", Pattern::Noise),
                    pattern_option("pattern-cycle", "Colour cycle", Pattern::Cycle),
                ]),
            )
            .into_any_element(),
            row("Stop after", run_length).into_any_element(),
            row_with_note("Clear image retention", note, action).into_any_element(),
        ])
    }

    fn render_footer(&self, status: Option<&ServiceStatus>, cx: &Context<Self>) -> Div {
        let notices: Vec<Notice> = self
            .notice
            .iter()
            .cloned()
            .chain(status.map(status_notices).unwrap_or_default())
            .collect();

        div()
            .flex()
            .flex_none()
            .flex_col()
            .border_t_1()
            .border_color(theme::rule())
            .children(notices.into_iter().map(notice_line))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(40.0))
                    .px(px(14.0))
                    .child(button(
                        "reload",
                        "Reload config",
                        ButtonKind::Quiet,
                        status.is_some(),
                        self.on_click(cx, Self::reload),
                    ))
                    .child(
                        div()
                            .pr(px(6.0))
                            .text_xs()
                            .text_color(theme::text_faint())
                            .child(VERSION),
                    ),
            )
    }
}

fn threshold_caption(status: &ServiceStatus, limit: u32) -> String {
    match status.applied_threshold {
        Some(threshold) if status.boost_until.is_some() => {
            format!("Charging to {threshold}% for the boost, limit {limit}%")
        }

        Some(threshold) if status.calibration_active => {
            format!("Charging to {threshold}% to calibrate, limit {limit}%")
        }

        Some(threshold) if threshold == limit => format!("Charging stops at {limit}%"),

        Some(threshold) => format!("Threshold {threshold}%, limit {limit}%"),

        None => format!("Limit {limit}%, threshold not reported"),
    }
}

fn fault_lines(fault: &Fault) -> Vec<AnyElement> {
    vec![
        div()
            .text_sm()
            .text_color(theme::text())
            .child(SharedString::from(fault.cause.clone()))
            .into_any_element(),
        div()
            .text_xs()
            .text_color(theme::text_faint())
            .child(SharedString::from(fault.hint.clone()))
            .into_any_element(),
    ]
}

fn notice_line(notice: Notice) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .px(px(20.0))
        .pt(px(10.0))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(severity_color(notice.severity))
                .child(notice.headline),
        )
        .when(!notice.detail.is_empty(), |this| {
            this.child(
                div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(notice.detail),
            )
        })
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.link.status();

        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::canvas())
            .text_color(theme::text())
            .child(
                div()
                    .id("body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.render_battery())
                    .child(self.render_charge(status, cx))
                    .child(self.render_full_charge(status, cx))
                    .child(self.render_wifi(status, cx))
                    .child(self.render_pixel_refresh(cx)),
            )
            .child(self.render_footer(status, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(limit: u32, threshold: Option<u32>) -> ServiceStatus {
        ServiceStatus {
            charge_limit: limit,
            enable_periodic_full_charge: true,
            full_charge_period: 30,
            battery_health: Some(94),
            battery_charge: Some(72),
            boost_until: None,
            applied_threshold: threshold,
            last_full_charge: None,
            calibration_active: false,
            full_charge_hold: Some(60),
            held_at_full: None,
            threshold_error: None,
            config_error: None,
            wifi_power_save_on_ac: Some(true),
        }
    }

    #[test]
    fn caption_names_why_the_threshold_differs() {
        let mut boosting = status(80, Some(100));
        boosting.boost_until = Some(1);

        let mut calibrating = status(80, Some(100));
        calibrating.calibration_active = true;

        assert_eq!(
            threshold_caption(&status(80, Some(80)), 80),
            "Charging stops at 80%"
        );
        assert_eq!(
            threshold_caption(&boosting, 80),
            "Charging to 100% for the boost, limit 80%"
        );
        assert_eq!(
            threshold_caption(&calibrating, 80),
            "Charging to 100% to calibrate, limit 80%"
        );
        assert_eq!(
            threshold_caption(&status(80, None), 80),
            "Limit 80%, threshold not reported"
        );
    }

    #[test]
    fn fields_round_trip_through_the_status() {
        let mut status = status(80, Some(80));

        Field::Hold.write(&mut status, 90);
        Field::Period.write(&mut status, 14);

        assert_eq!(Field::Hold.read(&status), Some(90));
        assert_eq!(Field::Period.read(&status), Some(14));
        assert_eq!(Field::Period.display(1), "1 day");
        assert_eq!(Field::Limit.request(85), Request::SetChargeLimit(85));
    }
}
