pub mod ai_prompt;
pub mod covers;
mod timer;

use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div,
};
use gpui_component::input::{InputState, TextareaState};
use ui_components::settings::{SettingPage, Settings};
use ui_resources::i18n::tools_strings;

use crate::localization::LangChanged;
use crate::services::Services;

use ai_prompt::{AiPromptInputs, AiPromptState, Mode};
use covers::CoversState;

use crate::sleep_timer::SleepTimer;
use crate::sleep_timer::controls::SleepTimerControls;

const WISHES_ROWS: (usize, usize) = (2, 6);
const ANSWER_ROWS: (usize, usize) = (4, 12);
pub const TIMER_PAGE: usize = 2;

pub struct ToolsView {
    ai_prompt: Entity<AiPromptState>,
    inputs: AiPromptInputs,
    mode: Mode,
    covers: Entity<CoversState>,
    covers_layout: covers::Layout,
    sleep_timer: Option<Entity<SleepTimer>>,
    sleep_timer_controls: Entity<SleepTimerControls>,
    pages: Vec<SettingPage>,
    page_ix: usize,
    page_request: u64,
    _ai_prompt_observe: Subscription,
    _covers_observe: Subscription,
    _sleep_timer_observe: Option<Subscription>,
    _sleep_timer_controls_observe: Subscription,
    _lang_subscription: Subscription,
}

impl ToolsView {
    pub fn new(
        sleep_timer_controls: Entity<SleepTimerControls>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ai_prompt = cx.new(|_| AiPromptState::default());
        let covers = cx.new(|_| CoversState::default());
        let wishes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(WISHES_ROWS.0, WISHES_ROWS.1)
                .placeholder(tools_strings().ai_prompt_wishes_placeholder.clone())
        });
        let answer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(ANSWER_ROWS.0, ANSWER_ROWS.1)
                .placeholder(tools_strings().ai_answer_text.clone())
        });
        let playlist_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tools_strings().ai_answer_name_placeholder.clone())
        });
        let inputs = AiPromptInputs {
            wishes,
            answer,
            playlist_name,
        };
        let ai_prompt_observe = cx.observe(&ai_prompt, |this, state, cx| {
            let mode = state.read(cx).mode();
            if mode != this.mode {
                this.mode = mode;
                this.pages = this.build_pages();
            }
            cx.notify();
        });
        let covers_observe = cx.observe(&covers, |this, state, cx| {
            let layout = state.read(cx).layout();
            if layout != this.covers_layout {
                this.covers_layout = layout;
                this.pages = this.build_pages();
            }
            cx.notify();
        });
        let lang_bus = cx.global::<Services>().lang_event_bus.clone();
        let lang_subscription =
            cx.subscribe_in(&lang_bus, window, |this, _, _: &LangChanged, window, cx| {
                let s = tools_strings();
                this.inputs.wishes.update(cx, |state, cx| {
                    state.set_placeholder(s.ai_prompt_wishes_placeholder.clone(), window, cx)
                });
                this.inputs.answer.update(cx, |state, cx| {
                    state.set_placeholder(s.ai_answer_text.clone(), window, cx)
                });
                this.inputs.playlist_name.update(cx, |state, cx| {
                    state.set_placeholder(s.ai_answer_name_placeholder.clone(), window, cx)
                });
                this.covers.update(cx, |state, _| state.relabel());
                this.pages = this.build_pages();
                cx.notify();
            });
        let mode = ai_prompt.read(cx).mode();
        let covers_layout = covers.read(cx).layout();
        let sleep_timer = crate::sleep_timer::timer(cx);
        let sleep_timer_observe = sleep_timer
            .as_ref()
            .map(|timer| cx.observe(timer, |_, _, cx| cx.notify()));
        let sleep_timer_controls_observe =
            cx.observe(&sleep_timer_controls, |_, _, cx| cx.notify());
        let mut view = Self {
            ai_prompt,
            inputs,
            mode,
            covers,
            covers_layout,
            sleep_timer,
            sleep_timer_controls,
            pages: Vec::new(),
            page_ix: 0,
            page_request: 0,
            _ai_prompt_observe: ai_prompt_observe,
            _covers_observe: covers_observe,
            _sleep_timer_observe: sleep_timer_observe,
            _sleep_timer_controls_observe: sleep_timer_controls_observe,
            _lang_subscription: lang_subscription,
        };
        view.pages = view.build_pages();
        view
    }

    fn build_pages(&self) -> Vec<SettingPage> {
        let mut pages = vec![
            ai_prompt::page(self.ai_prompt.clone(), self.inputs.clone(), self.mode),
            covers::page(self.covers.clone(), self.covers_layout),
        ];
        if let Some(timer) = &self.sleep_timer {
            pages.push(timer::page(
                timer.clone(),
                self.sleep_timer_controls.clone(),
            ));
        }
        pages
    }

    pub fn show_page(&mut self, page_ix: usize, cx: &mut Context<Self>) {
        self.page_ix = page_ix;
        self.page_request += 1;
        cx.notify();
    }
}

impl Render for ToolsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Settings::new("pawse-tools")
                .initial_page(self.page_ix)
                .page_request(self.page_request)
                .pages(self.pages.clone()),
        )
    }
}

pub fn title() -> gpui::SharedString {
    tools_strings().tools.clone()
}
