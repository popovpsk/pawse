pub mod ai_prompt;

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

const WISHES_ROWS: (usize, usize) = (2, 6);
const ANSWER_ROWS: (usize, usize) = (4, 12);

pub struct ToolsView {
    ai_prompt: Entity<AiPromptState>,
    inputs: AiPromptInputs,
    mode: Mode,
    pages: Vec<SettingPage>,
    _ai_prompt_observe: Subscription,
    _lang_subscription: Subscription,
}

impl ToolsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ai_prompt = cx.new(|_| AiPromptState::default());
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
                this.pages = build_pages(&this.ai_prompt, &this.inputs, mode);
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
                this.pages = build_pages(&this.ai_prompt, &this.inputs, this.mode);
                cx.notify();
            });
        let mode = ai_prompt.read(cx).mode();
        let pages = build_pages(&ai_prompt, &inputs, mode);
        Self {
            ai_prompt,
            inputs,
            mode,
            pages,
            _ai_prompt_observe: ai_prompt_observe,
            _lang_subscription: lang_subscription,
        }
    }
}

fn build_pages(
    ai_prompt: &Entity<AiPromptState>,
    inputs: &AiPromptInputs,
    mode: Mode,
) -> Vec<SettingPage> {
    vec![ai_prompt::page(ai_prompt.clone(), inputs.clone(), mode)]
}

impl Render for ToolsView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(Settings::new("pawse-tools").pages(self.pages.clone()))
    }
}

pub fn title() -> gpui::SharedString {
    tools_strings().tools.clone()
}
