use gpui::{
    App, AppContext, Context, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    progress::Progress,
    v_flex,
};
use ui_resources::i18n::similar_strings;

use super::{Phase, Status};
use crate::theme_colors::Colors;

pub fn open(window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| {
        let mut view = IndexingView {
            shown: None,
            loading: false,
        };
        view.refresh(cx);
        view
    });
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let s = similar_strings();
        let refresh = view.clone();
        dialog
            .w(px(460.))
            .close_button(false)
            .title(s.indexing.clone())
            .child(view.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("similar-indexing-refresh")
                            .label(s.refresh.clone())
                            .on_click(move |_, _, cx| {
                                refresh.update(cx, |view, cx| view.refresh(cx));
                            }),
                    )
                    .child(
                        Button::new("similar-indexing-close")
                            .label(s.close.clone())
                            .primary()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

struct IndexingView {
    shown: Option<Shown>,
    loading: bool,
}

struct Shown {
    headline: SharedString,
    percent: Option<f32>,
    phase: Option<SharedString>,
    notes: Vec<SharedString>,
}

impl IndexingView {
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let Some(similar) = super::current(cx) else {
            self.shown = Some(Shown::message(similar_strings().off.clone()));
            cx.notify();
            return;
        };
        self.loading = true;
        let task = cx.background_spawn(async move { similar.status() });
        cx.spawn(async move |this, cx| {
            let status = task.await;
            let _ = this.update(cx, |view, cx| {
                view.loading = false;
                view.shown = Some(match status {
                    Ok(status) => Shown::of(&status),
                    Err(e) => {
                        log::error!("similar tracks: could not read the progress: {e}");
                        Shown::message(similar_strings().status_failed.clone())
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }
}

impl Shown {
    fn message(headline: SharedString) -> Self {
        Self {
            headline,
            percent: None,
            phase: None,
            notes: Vec::new(),
        }
    }

    fn of(status: &Status) -> Self {
        let s = similar_strings();
        let percent = if status.total == 0 {
            0.0
        } else {
            status.analysed as f32 * 100.0 / status.total as f32
        };
        let phase = match status.phase {
            Phase::Idle => &s.idle,
            Phase::LoadingModel => &s.loading_model,
            Phase::Analysing => &s.analysing,
            Phase::NoModel => &s.no_model,
            Phase::Stopped => &s.stopped,
        };
        let mut notes = Vec::new();
        if status.failed > 0 {
            notes.push(s.failed_count(status.failed).into());
        }
        if status.unreachable > 0 {
            notes.push(s.unreachable(status.unreachable).into());
        }
        Self {
            headline: s.analysed(status.analysed, status.total).into(),
            percent: Some(percent),
            phase: Some(phase.clone()),
            notes,
        }
    }
}

impl Render for IndexingView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = Colors::muted_foreground(cx);
        let s = similar_strings();
        let about = div().text_sm().text_color(muted).child(s.about.clone());
        let Some(shown) = &self.shown else {
            return v_flex()
                .gap_3()
                .child(about)
                .child(div().text_color(muted).child(s.loading.clone()));
        };
        v_flex()
            .gap_3()
            .child(about)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(shown.headline.clone()),
            )
            .when_some(shown.percent, |el, percent| {
                el.child(Progress::new("similar-indexing-progress").value(percent))
            })
            .when_some(shown.phase.clone(), |el, phase| {
                el.child(div().child(phase))
            })
            .children(
                shown
                    .notes
                    .iter()
                    .map(|note| div().text_sm().text_color(muted).child(note.clone())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        phase: Phase,
        analysed: usize,
        total: usize,
        failed: usize,
        unreachable: usize,
    ) -> Status {
        Status {
            phase,
            analysed,
            total,
            failed,
            unreachable,
        }
    }

    #[test]
    fn the_dialog_shows_the_count_the_share_and_only_the_notes_that_apply() {
        let shown = Shown::of(&status(Phase::Analysing, 10, 2000, 0, 0));
        assert_eq!(
            shown.headline,
            SharedString::from(similar_strings().analysed(10, 2000))
        );
        assert_eq!(shown.percent, Some(0.5));
        assert_eq!(shown.phase, Some(similar_strings().analysing.clone()));
        assert!(shown.notes.is_empty());

        let shown = Shown::of(&status(Phase::Idle, 5, 9, 1, 3));
        assert_eq!(shown.notes.len(), 2);
        assert_eq!(
            Shown::of(&status(Phase::Idle, 0, 0, 0, 0)).percent,
            Some(0.0)
        );
    }
}
