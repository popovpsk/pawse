use gpui::prelude::FluentBuilder;
use gpui::{
    AppContext, Context, ElementId, Entity, EventEmitter, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px, svg,
};
use gpui_component::{
    Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use nucleo_matcher::{Config, Matcher};

use crate::library_service::LibraryEvent;
use crate::library_views::fuzzy::fuzzy_sorted;
use crate::localization::tr;
use crate::services::Services;
use crate::theme_colors::Colors;
use crate::track_list::LIKE_ROW_GROUP;

#[derive(Clone, Debug)]
pub struct PlaylistSelectedEvent {
    pub playlist: music_library::PlaylistSummary,
}

pub struct PlaylistsView {
    playlists_all: Vec<music_library::PlaylistSummary>,
    playlists: Vec<music_library::PlaylistSummary>,
    filter: String,
    matcher: Matcher,
    creating: bool,
    create_input: Entity<InputState>,
    pending_delete_id: Option<i64>,
    _library_subscription: Subscription,
    _create_subscription: Subscription,
}

impl PlaylistsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library_event_bus = cx.global::<Services>().library_event_bus.clone();

        let create_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tr().new_playlist_name.clone())
                .clean_on_escape()
        });

        let create_subscription = cx.subscribe(&create_input, |this, _, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.commit_create(cx);
            }
        });

        let playlists_all = cx.global::<Services>().library.playlists();
        let playlists = playlists_all.clone();

        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                let refresh = match event {
                    LibraryEvent::PlaylistsChanged => true,
                    LibraryEvent::ScanComplete { changed } => *changed,
                    _ => false,
                };
                if refresh {
                    let services = cx.global::<Services>();
                    this.playlists_all = services.library.playlists();
                    this.recompute_visible();
                    cx.notify();
                }
            });

        Self {
            playlists_all,
            playlists,
            filter: String::new(),
            matcher: Matcher::new(Config::DEFAULT),
            creating: false,
            create_input,
            pending_delete_id: None,
            _library_subscription: library_subscription,
            _create_subscription: create_subscription,
        }
    }

    pub fn set_filter(&mut self, query: &str, cx: &mut Context<Self>) {
        let trimmed = query.trim().to_string();
        if trimmed == self.filter {
            return;
        }
        self.filter = trimmed;
        self.recompute_visible();
        cx.notify();
    }

    fn recompute_visible(&mut self) {
        if self.filter.is_empty() {
            self.playlists = self.playlists_all.clone();
            return;
        }
        let indices = fuzzy_sorted(
            &mut self.matcher,
            &self.filter,
            self.playlists_all
                .iter()
                .enumerate()
                .map(|(ix, p)| (ix, p.name.as_str())),
        );
        self.playlists = indices
            .into_iter()
            .map(|ix| self.playlists_all[ix].clone())
            .collect();
    }

    fn commit_create(&mut self, cx: &mut Context<Self>) {
        let name = self.create_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        cx.global::<Services>().library.create_playlist(&name);
        self.creating = false;
        cx.notify();
    }
}

impl EventEmitter<PlaylistSelectedEvent> for PlaylistsView {}

impl Render for PlaylistsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = Colors::panel_border(cx);
        let list_hover = Colors::list_row_hover_bg(cx);
        let muted_fg = Colors::text_secondary(cx);
        let danger_fg = Colors::text_primary(cx);
        let icon_btn_hover = Colors::icon_button_hover_bg(cx);

        let create_section = if self.creating {
            v_flex().px_4().py_3().child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(240.))
                            .child(Input::new(&self.create_input).small().cleanable(false)),
                    )
                    .child(
                        Button::new("playlists-confirm-create")
                            .primary()
                            .compact()
                            .label(tr().create.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.commit_create(cx);
                            })),
                    )
                    .child(
                        Button::new("playlists-cancel-create")
                            .ghost()
                            .compact()
                            .label(tr().cancel.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.creating = false;
                                cx.notify();
                            })),
                    ),
            )
        } else {
            v_flex().px_4().pt_3().pb_2().child(
                h_flex().child(
                    Button::new("playlists-new")
                        .outline()
                        .compact()
                        .label(tr().new_playlist.clone())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.creating = true;
                            this.create_input.update(cx, |s, cx| {
                                s.set_value("", window, cx);
                                s.focus(window, cx);
                            });
                            cx.notify();
                        })),
                ),
            )
        };

        let mut list = v_flex().w_full();

        if self.playlists.is_empty() {
            let message = if self.playlists_all.is_empty() {
                tr().no_playlists_yet.clone()
            } else {
                tr().no_playlists_match.clone()
            };
            list = list.child(
                div()
                    .px_4()
                    .py_2()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(message),
            );
        } else {
            for p in self.playlists.iter() {
                let playlist = p.clone();
                let playlist_id = playlist.id;
                let count_str = tr().n_tracks(playlist.track_count);
                let pending_delete = self.pending_delete_id == Some(playlist_id);

                let trash_button = div()
                    .id(ElementId::NamedInteger(
                        "pl-trash".into(),
                        playlist_id as u64,
                    ))
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .when(!pending_delete, |d| {
                        d.opacity(0.).group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
                    })
                    .hover(|s| s.bg(icon_btn_hover))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pending_delete_id = Some(playlist_id);
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path("icons/s1-trash.svg")
                            .size(px(15.))
                            .text_color(danger_fg),
                    );

                let row = h_flex()
                    .group(LIKE_ROW_GROUP)
                    .w_full()
                    .h(px(48.))
                    .px_4()
                    .gap_3()
                    .items_center()
                    .border_b(px(1.))
                    .border_color(border)
                    .hover(|s| s.bg(list_hover))
                    .child(
                        svg()
                            .path("icons/s1-playlists.svg")
                            .size(px(20.))
                            .text_color(danger_fg),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .truncate()
                            .child(playlist.name.clone()),
                    )
                    .child(div().text_sm().text_color(muted_fg).child(count_str))
                    .when(pending_delete, |row| {
                        let pid = playlist_id;
                        // Wrap the confirm/cancel buttons in a div that stops
                        // mouse-down propagation, so the click doesn't bubble
                        // up to the row and trigger drill-in.
                        row.child(
                            h_flex()
                                .gap_2()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    Button::new(ElementId::NamedInteger(
                                        "pl-del-confirm".into(),
                                        pid as u64,
                                    ))
                                    .danger()
                                    .compact()
                                    .label(tr().delete.clone())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.global::<Services>().library.delete_playlist(pid);
                                            this.pending_delete_id = None;
                                        },
                                    )),
                                )
                                .child(
                                    Button::new(ElementId::NamedInteger(
                                        "pl-del-cancel".into(),
                                        pid as u64,
                                    ))
                                    .ghost()
                                    .compact()
                                    .label(tr().cancel.clone())
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.pending_delete_id = None;
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                    })
                    .when(!pending_delete, |row| row.child(trash_button))
                    .id(ElementId::Integer(playlist_id as u64))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.pending_delete_id.is_some() {
                            return;
                        }
                        cx.emit(PlaylistSelectedEvent {
                            playlist: playlist.clone(),
                        });
                    }));

                list = list.child(row);
            }
        }

        v_flex().size_full().child(create_section).child(list)
    }
}
