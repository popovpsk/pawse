use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, prelude::FluentBuilder, px, svg,
};
use gpui_component::{ActiveTheme, v_flex};

pub const SEARCH_EMPTY_ICON: &str = "icons/search-x.svg";

const ICON_SIZE: f32 = 40.;
const COMPACT_ICON_SIZE: f32 = 24.;
const TEXT_MAX_WIDTH: f32 = 360.;

#[derive(IntoElement)]
pub struct EmptyState {
    icon: &'static str,
    title: SharedString,
    hint: Option<SharedString>,
    action: Option<AnyElement>,
    compact: bool,
}

impl EmptyState {
    pub fn new(icon: &'static str, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            hint: None,
            action: None,
            compact: false,
        }
    }

    pub fn search(title: impl Into<SharedString>) -> Self {
        Self::new(SEARCH_EMPTY_ICON, title)
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let compact = self.compact;
        let icon_size = if compact {
            COMPACT_ICON_SIZE
        } else {
            ICON_SIZE
        };
        let title = div()
            .max_w(px(TEXT_MAX_WIDTH))
            .text_center()
            .map(|d| {
                if compact {
                    d.text_sm().text_color(muted)
                } else {
                    d.font_weight(FontWeight::SEMIBOLD)
                }
            })
            .child(self.title);
        let content = v_flex()
            .flex_shrink_0()
            .items_center()
            .map(|d| if compact { d.gap_2() } else { d.gap_3() })
            .child(svg().path(self.icon).size(px(icon_size)).text_color(muted))
            .child(title)
            .when_some(self.hint, |d, hint| {
                d.child(
                    div()
                        .max_w(px(TEXT_MAX_WIDTH))
                        .text_sm()
                        .text_center()
                        .text_color(muted)
                        .child(hint),
                )
            })
            .when_some(self.action, |d, action| d.child(div().pt_2().child(action)));
        if compact {
            return content.w_full().p_4().into_any_element();
        }
        v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .px_8()
            .pb_16()
            .child(div().flex_1())
            .child(content)
            .child(div().flex_1())
            .into_any_element()
    }
}
