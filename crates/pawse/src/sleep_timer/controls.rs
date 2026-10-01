use gpui::{App, AppContext, Context, Entity, SharedString, Subscription, Window};
use gpui_component::{
    input::{InputEvent, InputState, StepAction},
    slider::{SliderEvent, SliderState},
    time_field::{TimeFieldEvent, TimeFieldState},
};
use ui_resources::i18n::tools_strings;

use crate::localization::LangChanged;
use crate::services::Services;
use crate::settings_store::{
    SLEEP_TIMER_FADE_STEPS, SLEEP_TIMER_MAX_MIN, SettingsStore, sleep_timer_fade_step,
};

use super::schedule;
use super::settings::update;

const DURATION_STEP_MIN: u32 = 5;
const MANUAL_DEFAULT_MIN: u32 = 20;

pub struct SleepTimerControls {
    pub from: Entity<TimeFieldState>,
    pub until: Entity<TimeFieldState>,
    pub auto_duration: Entity<InputState>,
    pub manual_duration: Entity<InputState>,
    pub fade: Entity<SliderState>,
    manual_minutes: u32,
    fade_label: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl SleepTimerControls {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.global::<SettingsStore>().sleep_timer();
        let from = time_state(settings.auto_from_min, window, cx);
        let until = time_state(settings.auto_until_min, window, cx);
        let auto_duration = duration_input(settings.auto_duration_min, window, cx);
        let manual_duration = duration_input(MANUAL_DEFAULT_MIN, window, cx);
        let fade = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max((SLEEP_TIMER_FADE_STEPS.len() - 1) as f32)
                .step(1.)
                .default_value(sleep_timer_fade_step(settings.fade_secs) as f32)
        });
        let lang_bus = cx.global::<Services>().lang_event_bus.clone();
        let subscriptions = vec![
            cx.observe(&from, |_, _, cx| cx.notify()),
            cx.observe(&until, |_, _, cx| cx.notify()),
            cx.observe(&fade, |_, _, cx| cx.notify()),
            cx.subscribe(&from, |_, _, event: &TimeFieldEvent, cx| {
                let TimeFieldEvent::Change(time) = *event;
                update(cx, |s| s.auto_from_min = schedule::minute_of_day(time));
            }),
            cx.subscribe(&until, |_, _, event: &TimeFieldEvent, cx| {
                let TimeFieldEvent::Change(time) = *event;
                update(cx, |s| s.auto_until_min = schedule::minute_of_day(time));
            }),
            cx.subscribe_in(
                &auto_duration,
                window,
                |_, input, event: &InputEvent, window, cx| {
                    let stored = cx.global::<SettingsStore>().sleep_timer().auto_duration_min;
                    match event {
                        InputEvent::Change => {
                            let minutes = parse_minutes(&input.read(cx).value());
                            if let Some(minutes) = minutes
                                && minutes != stored
                            {
                                update(cx, |s| s.auto_duration_min = minutes);
                            }
                        }
                        InputEvent::Blur | InputEvent::PressEnter { .. } => {
                            let minutes = committed_minutes(&input.read(cx).value(), stored);
                            if minutes != stored {
                                update(cx, |s| s.auto_duration_min = minutes);
                            }
                            show_minutes(input, minutes, window, cx);
                        }
                        InputEvent::Focus => {}
                    }
                },
            ),
            cx.subscribe_in(
                &manual_duration,
                window,
                |this, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        if let Some(minutes) = parse_minutes(&input.read(cx).value()) {
                            this.manual_minutes = minutes;
                        }
                    }
                    InputEvent::Blur | InputEvent::PressEnter { .. } => {
                        this.manual_minutes =
                            committed_minutes(&input.read(cx).value(), this.manual_minutes);
                        show_minutes(input, this.manual_minutes, window, cx);
                        if matches!(event, InputEvent::PressEnter { .. }) {
                            let minutes = this.manual_minutes;
                            if let Some(timer) = super::timer(cx) {
                                timer.update(cx, |t, cx| t.start(minutes, cx));
                            }
                        }
                    }
                    InputEvent::Focus => {}
                },
            ),
            cx.subscribe(&fade, |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(value) = event else {
                    return;
                };
                let secs = fade_secs_at(value.start());
                if secs != cx.global::<SettingsStore>().sleep_timer().fade_secs {
                    update(cx, |s| s.fade_secs = secs);
                }
                this.relabel(cx);
                cx.notify();
            }),
            cx.subscribe(&lang_bus, |this, _, _: &LangChanged, cx| {
                this.relabel(cx);
                cx.notify();
            }),
        ];
        let mut controls = Self {
            from,
            until,
            auto_duration,
            manual_duration,
            fade,
            manual_minutes: MANUAL_DEFAULT_MIN,
            fade_label: SharedString::default(),
            _subscriptions: subscriptions,
        };
        controls.relabel(cx);
        controls
    }

    pub fn fade_label(&self) -> SharedString {
        self.fade_label.clone()
    }

    pub fn manual_minutes(&self) -> u32 {
        self.manual_minutes
    }

    fn relabel(&mut self, cx: &App) {
        let secs = fade_secs_at(self.fade.read(cx).value().start());
        self.fade_label = SharedString::from(fade_label(secs));
    }
}

fn fade_secs_at(position: f32) -> u32 {
    let ix = (position.round().max(0.) as usize).min(SLEEP_TIMER_FADE_STEPS.len() - 1);
    SLEEP_TIMER_FADE_STEPS[ix]
}

fn fade_label(secs: u32) -> String {
    let s = tools_strings();
    if secs < 120 {
        s.timer_seconds(secs)
    } else {
        s.timer_minutes(secs / 60)
    }
}

fn time_state(minute_of_day: u16, window: &mut Window, cx: &mut App) -> Entity<TimeFieldState> {
    cx.new(|cx| {
        let mut state = TimeFieldState::new(window, cx);
        state.set_time(schedule::time_of_day(minute_of_day), window, cx);
        state
    })
}

fn duration_input(minutes: u32, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(minutes.to_string())
            .validate(|text, _| text.chars().all(|c| c.is_ascii_digit()))
            .step_by(|value, action, _| duration_step(value, action))
            .min(1.)
            .max(f64::from(SLEEP_TIMER_MAX_MIN))
    })
}

fn show_minutes(input: &Entity<InputState>, minutes: u32, window: &mut Window, cx: &mut App) {
    let text = minutes.to_string();
    if input.read(cx).value().as_ref() != text.as_str() {
        input.update(cx, |state, cx| state.set_value(text, window, cx));
    }
}

fn parse_minutes(text: &str) -> Option<u32> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|minutes| (1..=SLEEP_TIMER_MAX_MIN).contains(minutes))
}

fn committed_minutes(text: &str, fallback: u32) -> u32 {
    text.trim()
        .parse::<u64>()
        .map(|minutes| minutes.clamp(1, u64::from(SLEEP_TIMER_MAX_MIN)) as u32)
        .unwrap_or(fallback)
}

fn duration_step(value: f64, action: StepAction) -> f64 {
    let step = f64::from(DURATION_STEP_MIN);
    let offset = value.rem_euclid(step);
    match action {
        StepAction::Increment => step - offset,
        StepAction::Decrement if offset > 0. => offset,
        StepAction::Decrement => step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::up_from_round(20., StepAction::Increment, 25.)]
    #[case::up_snaps(22., StepAction::Increment, 25.)]
    #[case::up_from_one(1., StepAction::Increment, 5.)]
    #[case::down_from_round(20., StepAction::Decrement, 15.)]
    #[case::down_snaps(22., StepAction::Decrement, 20.)]
    fn steps_snap_to_five_minutes(
        #[case] value: f64,
        #[case] action: StepAction,
        #[case] expected: f64,
    ) {
        let delta = duration_step(value, action);
        let next = match action {
            StepAction::Increment => value + delta,
            StepAction::Decrement => value - delta,
        };
        assert_eq!(next, expected);
    }

    #[rstest]
    #[case::plain("45", Some(45))]
    #[case::padded(" 7 ", Some(7))]
    #[case::zero("0", None)]
    #[case::too_long("721", None)]
    #[case::empty("", None)]
    #[case::not_a_number("1.5", None)]
    fn minutes_parse(#[case] text: &str, #[case] expected: Option<u32>) {
        assert_eq!(parse_minutes(text), expected);
    }

    #[rstest]
    #[case::left_end(0., 5)]
    #[case::default_position(5., 30)]
    #[case::between_positions(11.4, 120)]
    #[case::right_end(25., 3600)]
    #[case::past_the_ends(-3., 5)]
    #[case::past_the_right_end(99., 3600)]
    fn slider_positions_map_to_steps(#[case] position: f32, #[case] secs: u32) {
        assert_eq!(fade_secs_at(position), secs);
    }

    #[test]
    fn steps_shown_in_minutes_are_whole_minutes() {
        assert!(
            SLEEP_TIMER_FADE_STEPS
                .iter()
                .filter(|secs| **secs >= 120)
                .all(|secs| secs % 60 == 0)
        );
    }

    #[rstest]
    #[case::plain("45", 45)]
    #[case::too_long("1000", SLEEP_TIMER_MAX_MIN)]
    #[case::zero("0", 1)]
    #[case::empty("", 20)]
    #[case::not_a_number("1.5", 20)]
    fn committed_minutes_clamp_or_fall_back(#[case] text: &str, #[case] expected: u32) {
        assert_eq!(committed_minutes(text, 20), expected);
    }
}
