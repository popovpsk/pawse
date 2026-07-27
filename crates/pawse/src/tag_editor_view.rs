use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::StyledImage;
use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Context, ElementId, Entity, Image, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, RenderImage, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, rems, svg,
};
use gpui_component::{
    Sizable, WindowExt,
    button::Button,
    dialog::DialogButtonProps,
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    v_flex,
};

use crate::library_service::AlbumTagEdits;
use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;
use ui_components::cover_thumb::cover_thumb;
use ui_components::scrollbar_metrics::SCROLLBAR_GUTTER;

const DIALOG_WIDTH: f32 = 560.;
const LIST_SEPARATOR: &str = "; ";
const LABEL_WIDTH: f32 = 132.;
const CHIP_BUTTON_SIZE: f32 = 20.;
const CHIP_ICON_SIZE: f32 = 12.;
const ACTION_BUTTON_SIZE: f32 = 26.;
const ACTION_ICON_SIZE: f32 = 15.;
/// `gpui_component`'s dialog is top-anchored at `viewport_height / 10` rather than
/// centered, and pads itself by 24px — the fractions/sizes here mirror that so the
/// form's cap can be derived instead of guessed.
const DIALOG_TOP_FRACTION: f32 = 0.1;
/// The dialog's own 24px top+bottom padding plus its title row and button footer.
const DIALOG_CHROME_HEIGHT: f32 = 144.;
/// Breathing room between the dialog's bottom edge and the player footer.
const DIALOG_BOTTOM_MARGIN: f32 = 24.;
const MIN_FORM_HEIGHT: f32 = 120.;
const COVER_PREVIEW_SIZE: f32 = 84.;
const COVER_PREVIEW_RADIUS: f32 = 6.;
/// List rows get their breathing room from their own horizontal padding; the form has
/// none, so it adds this on top of [`SCROLLBAR_GUTTER`].
const FORM_RIGHT_GAP: f32 = 4.;

enum Target {
    Track { track_id: i64, has_album: bool },
    Album { album_id: i64 },
}

pub struct TagEditorView {
    target: Target,
    read_only: bool,
    title: Entity<InputState>,
    artists: Entity<InputState>,
    album: Entity<InputState>,
    album_artists: Entity<InputState>,
    track_number: Entity<InputState>,
    disc_number: Entity<InputState>,
    year: Entity<InputState>,
    genres: Vec<SharedString>,
    genre_input: Entity<InputState>,
    raw_tags: Vec<(SharedString, SharedString)>,
    /// The file's rows as they were loaded. `save` diffs `raw_tags` against this
    /// instead of bookkeeping every click.
    original_raw: Vec<tag_writer::RawTag>,
    custom_tags: tag_writer::CustomTags,
    custom_key: Entity<InputState>,
    custom_value: Entity<InputState>,
    scroll: ScrollHandle,
    cover: tag_writer::CoverEdit,
    /// What the picture row shows: the file's own embedded tag when the dialog opened,
    /// then whatever the user picked instead. `None` means the file carries no picture —
    /// which is also the state where there is nothing for Remove to do.
    cover_preview: Option<(Arc<RenderImage>, usize)>,
    _genre_subscription: Subscription,
    _custom_subscription: Subscription,
}

pub fn open_for_track(track: Rc<music_library::Track>, window: &mut Window, cx: &mut App) {
    let path = PathBuf::from(&track.path);
    let Some(loaded) = load(&track, &path, true, cx) else {
        return;
    };
    let target = Target::Track {
        track_id: track.id,
        has_album: track.album_id.is_some(),
    };
    let view = cx.new(|cx| TagEditorView::new(target, track.is_cue, loaded, window, cx));
    open_dialog(view, tr().edit_tags.clone(), window, cx);
}

pub fn open_for_album(album_id: i64, window: &mut Window, cx: &mut App) {
    let library = cx.global::<Services>().library.clone();
    let tracks = library.tracks_for_album(album_id);
    let read_only = !tracks.iter().any(|t| !t.is_cue);
    let Some(track) = tracks.iter().find(|t| !t.is_cue).or_else(|| tracks.first()) else {
        return;
    };
    let Some(mut loaded) = load(track, Path::new(&track.path), false, cx) else {
        return;
    };
    loaded.album_artists = library.album_artists(album_id);
    loaded.genres = library.album_genres(album_id);
    let view =
        cx.new(|cx| TagEditorView::new(Target::Album { album_id }, read_only, loaded, window, cx));
    open_dialog(view, tr().edit_album_tags.clone(), window, cx);
}

struct Loaded {
    title: String,
    artists: Vec<String>,
    album: String,
    album_artists: Vec<String>,
    track_number: Option<u32>,
    disc_number: Option<u32>,
    year: Option<i32>,
    genres: Vec<String>,
    raw_tags: Vec<(SharedString, SharedString)>,
    custom_tags: tag_writer::CustomTags,
    /// The picture *embedded in the file*, decoded for the preview. Deliberately not the
    /// cover the library shows: that one may have been found in a neighbouring
    /// `cover.jpg`, and a tag editor claiming the file holds a tag it does not is a lie
    /// the user would act on — pressing Remove would appear to do something and change
    /// nothing.
    cover: Option<(Arc<RenderImage>, usize)>,
}

fn load(track: &music_library::Track, path: &Path, want_raw: bool, cx: &App) -> Option<Loaded> {
    let (raw_tags, custom_tags) = if want_raw {
        (
            tag_writer::read_raw_tags(path)
                .unwrap_or_default()
                .into_iter()
                .map(|t| (SharedString::from(t.key), SharedString::from(t.value)))
                .collect(),
            tag_writer::custom_tags_for(path),
        )
    } else {
        (Vec::new(), tag_writer::CustomTags::default())
    };

    if track.is_cue {
        let embedded = music_indexer::metadata::extract_embedded_cover(path);
        return Some(from_library(track, raw_tags, custom_tags, embedded, cx));
    }

    let scanned = match music_indexer::metadata::read_metadata(path) {
        Ok(scanned) => scanned,
        Err(e) => {
            log::error!("Failed to read tags of {}: {}", path.display(), e);
            diagnostics::notify_error(
                tr().tag_write_failed_title.to_string(),
                tr().tags_save_failed(&e.to_string()),
            );
            return None;
        }
    };
    Some(Loaded {
        title: scanned.title.unwrap_or_default(),
        artists: scanned.artist_names,
        album: scanned.album_title.unwrap_or_default(),
        album_artists: scanned.album_artist_names,
        track_number: scanned.track_number,
        disc_number: scanned.disc_number,
        year: scanned.year,
        genres: scanned.genres,
        raw_tags,
        custom_tags,
        cover: match scanned.cover_art {
            Some(music_indexer::CoverArt::Bytes {
                data,
                embedded: true,
                ..
            }) => decode_cover(&data, cx),
            _ => None,
        },
    })
}

/// Decode a picture for the preview, paired with its size in bytes.
///
/// `RenderImage` rather than `Image` because the sprite-atlas tile it takes can only be
/// handed back through `App::drop_image`, and the dialog opens often enough for one
/// leaked tile per open to add up.
fn decode_cover(bytes: &[u8], cx: &App) -> Option<(Arc<RenderImage>, usize)> {
    let format = crate::cover_mode_view::sniff_image_format(bytes)?;
    let image = Image::from_bytes(format, bytes.to_vec())
        .to_image_data(cx.svg_renderer())
        .ok()?;
    Some((image, bytes.len()))
}

fn from_library(
    track: &music_library::Track,
    raw_tags: Vec<(SharedString, SharedString)>,
    custom_tags: tag_writer::CustomTags,
    embedded_cover: Option<Vec<u8>>,
    cx: &App,
) -> Loaded {
    let library = &cx.global::<Services>().library;
    let album_id = track.album_id;
    Loaded {
        title: track.title.clone(),
        artists: library.track_artists(track.id),
        album: album_id
            .and_then(|id| library.album_title(id))
            .unwrap_or_default(),
        album_artists: album_id
            .map(|id| library.album_artists(id))
            .unwrap_or_default(),
        track_number: track.track_number.map(|n| n as u32),
        disc_number: Some(track.disc_number as u32),
        year: track.year,
        // A cue sheet carries one genre for the whole disc, so the album's set is
        // exactly what this track was indexed with.
        genres: album_id
            .map(|id| library.album_genres(id))
            .unwrap_or_default(),
        raw_tags,
        custom_tags,
        cover: embedded_cover.and_then(|bytes| decode_cover(&bytes, cx)),
    }
}

fn open_dialog(
    view: Entity<TagEditorView>,
    title: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    // The subscription is detached rather than held by the view it observes: it has to
    // outlive that view long enough to fire during its teardown.
    cx.observe_release(&view, |this, cx| this.release_preview(cx))
        .detach();
    window.open_dialog(cx, move |dialog, _, _| {
        let saver = view.clone();
        dialog
            .w(px(DIALOG_WIDTH))
            .close_button(false)
            .title(title.clone())
            .child(view.clone())
            .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
            .button_props(
                DialogButtonProps::default()
                    .ok_text(tr().tag_save.clone())
                    .cancel_text(tr().cancel.clone()),
            )
            .on_ok(move |_, window, cx| saver.update(cx, |this, cx| this.save(window, cx)))
    });
}

impl TagEditorView {
    fn new(
        target: Target,
        read_only: bool,
        loaded: Loaded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = input_field(loaded.title, window, cx);
        let artists = input_field(loaded.artists.join(LIST_SEPARATOR), window, cx);
        let album = input_field(loaded.album, window, cx);
        let album_artists = input_field(loaded.album_artists.join(LIST_SEPARATOR), window, cx);
        let track_number = input_field(opt_number(loaded.track_number), window, cx);
        let disc_number = input_field(opt_number(loaded.disc_number), window, cx);
        let year = input_field(opt_number(loaded.year), window, cx);

        let genre_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().tag_add_genre.clone()));
        let genre_subscription = cx.subscribe_in(
            &genre_input,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.commit_genre(window, cx);
                }
            },
        );

        let custom_key =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().tag_custom_key.clone()));
        let custom_value =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr().tag_custom_value.clone()));
        let custom_subscription = cx.subscribe_in(
            &custom_value,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.commit_custom(window, cx);
                }
            },
        );

        Self {
            title,
            artists,
            album,
            album_artists,
            track_number,
            disc_number,
            year,
            genres: loaded.genres.into_iter().map(SharedString::from).collect(),
            genre_input,
            original_raw: to_raw_tags(&loaded.raw_tags),
            custom_tags: loaded.custom_tags,
            raw_tags: loaded.raw_tags,
            custom_key,
            custom_value,
            scroll: ScrollHandle::new(),
            cover: tag_writer::CoverEdit::Keep,
            cover_preview: loaded.cover,
            target,
            read_only,
            _genre_subscription: genre_subscription,
            _custom_subscription: custom_subscription,
        }
    }

    fn read_only(&self) -> bool {
        self.read_only
    }

    /// Ask for an image file, keep it in memory until Save. The format is checked here
    /// rather than at write time: the writer runs on the background executor long after
    /// the dialog closed, so a refusal there would discard every other edit too.
    fn pick_cover(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("Image", &["jpg", "jpeg", "png"])
                .pick_file()
                .await
            else {
                return;
            };
            let path = handle.path().to_path_buf();
            let Ok(bytes) = cx
                .background_executor()
                .spawn(async move { std::fs::read(&path) })
                .await
            else {
                diagnostics::notify_error(
                    tr().tag_cover_failed_title.to_string(),
                    tr().tag_cover_unreadable.to_string(),
                );
                return;
            };
            if !tag_writer::cover_bytes_ok(&bytes) {
                diagnostics::notify_error(
                    tr().tag_cover_failed_title.to_string(),
                    tr().tag_cover_bad_format.to_string(),
                );
                return;
            }
            let decoded = decode_preview(&bytes, cx).await;
            let _ = this.update(cx, |this, cx| {
                this.release_preview(cx);
                this.cover_preview = decoded.map(|image| (image, bytes.len()));
                this.cover = tag_writer::CoverEdit::Replace { bytes };
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_cover(&mut self, cx: &mut Context<Self>) {
        self.release_preview(cx);
        self.cover = tag_writer::CoverEdit::Remove;
        cx.notify();
    }

    /// `drop_image` is the only thing that frees the sprite-atlas tile — `RenderImage` has
    /// no `Drop` that does it — so every preview has to be released explicitly, both when
    /// another file replaces it and when the dialog goes away.
    fn release_preview(&mut self, cx: &mut App) {
        if let Some((old, _)) = self.cover_preview.take() {
            cx.drop_image(old, None);
        }
    }

    /// Album, album artist and year all describe the shared `albums` row — the row
    /// is keyed on `(title, year)`, so editing either from a track that has siblings
    /// re-keys the album and splits it in two. Editable in the album editor, and in
    /// the track editor only for a track that belongs to no album at all: there is
    /// no shared row to break, and no album editor it could be reached from.
    fn album_fields_editable(&self) -> bool {
        match self.target {
            Target::Album { .. } => !self.read_only,
            Target::Track { has_album, .. } => !self.read_only && !has_album,
        }
    }

    fn track_fields_editable(&self) -> bool {
        !self.read_only && matches!(self.target, Target::Track { .. })
    }

    fn commit_genre(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.genre_input.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        let already = self
            .genres
            .iter()
            .any(|g| g.eq_ignore_ascii_case(value.as_str()));
        if !already {
            self.genres.push(SharedString::from(value));
        }
        self.genre_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    /// A container the writer cannot add rows to would offer an input that only ever
    /// produces an error toast.
    fn can_add_custom(&self) -> bool {
        !self.read_only() && self.custom_tags.supported()
    }

    fn commit_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.custom_key.read(cx).value().trim().to_string();
        let value = self.custom_value.read(cx).value().trim().to_string();
        if key.is_empty() || value.is_empty() {
            return;
        }
        if let Some(problem) = self.custom_tags.check_key(&key) {
            log::warn!("Custom tag rejected: {} ({:?})", key, problem);
            let message = match problem {
                tag_writer::KeyProblem::Invalid => tr().tag_custom_invalid.clone(),
                tag_writer::KeyProblem::FrameIdLength => tr().tag_custom_invalid_len.clone(),
            };
            diagnostics::notify_error(tr().tag_write_failed_title.to_string(), message.to_string());
            return;
        }

        let row = (SharedString::from(key), SharedString::from(value));
        if !self.raw_tags.contains(&row) {
            self.raw_tags.push(row);
        }

        self.custom_key
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.custom_value
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.read_only() {
            return true;
        }

        let track_number = match parse_number::<u32>(&self.track_number, cx) {
            Ok(value) => value,
            Err(()) => return self.reject(cx, tr().tag_track_number.clone()),
        };
        let disc_number = match parse_number::<u32>(&self.disc_number, cx) {
            Ok(value) => value,
            Err(()) => return self.reject(cx, tr().tag_disc_number.clone()),
        };
        let year = match parse_number::<i32>(&self.year, cx) {
            Ok(value) => value,
            Err(()) => return self.reject(cx, tr().tag_year.clone()),
        };

        let genres: Vec<String> = self.genres.iter().map(|g| g.to_string()).collect();
        let folders = cx.global::<SettingsStore>().music_folders().to_vec();
        let library = cx.global::<Services>().library.clone();

        match self.target {
            Target::Track { track_id, .. } => {
                let (removed_tags, added_tags) =
                    tag_writer::diff_raw_tags(&self.original_raw, &to_raw_tags(&self.raw_tags));
                let edits = tag_writer::TrackTagEdits {
                    title: text_of(&self.title, cx),
                    artists: split_list(&self.artists.read(cx).value()),
                    album: text_of(&self.album, cx),
                    album_artists: split_list(&self.album_artists.read(cx).value()),
                    track_number,
                    disc_number,
                    year,
                    genres,
                    removed_tags,
                    added_tags,
                    cover: self.cover.clone(),
                };
                library.update_track_tags(track_id, edits, folders);
            }
            Target::Album { album_id } => {
                let edits = AlbumTagEdits {
                    album: text_of(&self.album, cx),
                    album_artists: split_list(&self.album_artists.read(cx).value()),
                    year,
                    genres,
                    cover: self.cover.clone(),
                };
                library.update_album_tags(album_id, edits, folders);
            }
        }
        true
    }

    fn reject(&self, cx: &mut Context<Self>, field: SharedString) -> bool {
        log::warn!("Tag edit rejected: {} is not a number", field);
        diagnostics::notify_error(
            tr().tag_write_failed_title.to_string(),
            tr().tags_save_failed(&field),
        );
        cx.notify();
        false
    }
}

fn input_field(
    value: String,
    window: &mut Window,
    cx: &mut Context<TagEditorView>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}

fn opt_number<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn to_raw_tags(rows: &[(SharedString, SharedString)]) -> Vec<tag_writer::RawTag> {
    rows.iter()
        .map(|(key, value)| tag_writer::RawTag {
            key: key.to_string(),
            value: value.to_string(),
        })
        .collect()
}

fn text_of(state: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = state.read(cx).value();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_number<T: std::str::FromStr>(
    state: &Entity<InputState>,
    cx: &App,
) -> Result<Option<T>, ()> {
    let value = state.read(cx).value();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed.parse::<T>().map(Some).map_err(|_| ())
}

impl Render for TagEditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size().height;
        let reserved = viewport * DIALOG_TOP_FRACTION
            + px(crate::main_view::footer_height(cx) + DIALOG_BOTTOM_MARGIN + DIALOG_CHROME_HEIGHT);
        let max_height = (viewport - reserved).max(px(MIN_FORM_HEIGHT));

        div()
            .relative()
            .overflow_hidden()
            .max_h(max_height)
            .min_h(px(0.))
            .child(
                v_flex()
                    .id("tag-editor-form")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .pr(SCROLLBAR_GUTTER + px(FORM_RIGHT_GAP))
                    .child(self.form(window, cx)),
            )
            .vertical_scrollbar(&self.scroll)
    }
}

impl TagEditorView {
    fn cover_row(&self, muted: gpui::Hsla, cx: &mut Context<Self>) -> impl IntoElement {
        let removing = self.cover == tag_writer::CoverEdit::Remove;
        let shown = if removing {
            None
        } else {
            self.cover_preview.as_ref()
        };
        let is_album = matches!(self.target, Target::Album { .. });
        h_flex()
            .gap_3()
            .items_start()
            .child(
                div()
                    .w(px(LABEL_WIDTH))
                    .flex_shrink_0()
                    .pt_1()
                    .text_sm()
                    .text_color(muted)
                    .child(tr().tag_cover.clone()),
            )
            .child(match shown {
                Some((image, _)) => div()
                    .flex_shrink_0()
                    .child(
                        gpui::img(image.clone())
                            .size(px(COVER_PREVIEW_SIZE))
                            .rounded(px(COVER_PREVIEW_RADIUS))
                            .object_fit(gpui::ObjectFit::Cover),
                    )
                    .into_any_element(),
                None => cover_thumb(
                    None,
                    COVER_PREVIEW_SIZE,
                    COVER_PREVIEW_RADIUS,
                    Colors::muted(cx),
                    muted,
                ),
            })
            .when(!self.read_only(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("cover-replace")
                                        .small()
                                        .label(tr().tag_cover_replace.clone())
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.pick_cover(cx)),
                                        ),
                                )
                                // Nothing to remove when the file carries no picture, and a
                                // button that does nothing reads as a broken one.
                                .when(shown.is_some(), |el| {
                                    el.child(
                                        Button::new("cover-remove")
                                            .small()
                                            .label(tr().tag_cover_remove.clone())
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.remove_cover(cx)),
                                            ),
                                    )
                                }),
                        )
                        .when_some(shown, |el, (_, bytes)| {
                            el.child(div().text_xs().text_color(muted).child(format_size(*bytes)))
                        })
                        .when(is_album, |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(tr().tag_cover_album_hint.clone()),
                            )
                        })
                        .when(removing, |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(tr().tag_cover_will_remove.clone()),
                            )
                        }),
                )
            })
    }

    fn form(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = Colors::muted_foreground(cx);
        let border = Colors::border(cx);
        let track_fields = self.track_fields_editable();
        let album_fields = self.album_fields_editable();
        let is_track = matches!(self.target, Target::Track { .. });

        v_flex()
            .gap_3()
            .when(self.read_only(), |el| {
                el.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(tr().tag_cue_readonly.clone()),
                )
            })
            .child(self.cover_row(muted, cx))
            .when(is_track, |el| {
                el.child(field_row(
                    tr().tag_title.clone(),
                    Input::new(&self.title).small().disabled(!track_fields),
                    muted,
                ))
                .child(field_row(
                    tr().tag_artists.clone(),
                    Input::new(&self.artists).small().disabled(!track_fields),
                    muted,
                ))
            })
            .child(field_row(
                tr().tag_album.clone(),
                Input::new(&self.album).small().disabled(!album_fields),
                muted,
            ))
            .child(field_row(
                tr().tag_album_artists.clone(),
                Input::new(&self.album_artists)
                    .small()
                    .disabled(!album_fields),
                muted,
            ))
            .child(field_row(
                tr().tag_year.clone(),
                Input::new(&self.year).small().disabled(!album_fields),
                muted,
            ))
            .when(is_track && !self.read_only() && !album_fields, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(tr().tag_album_fields_hint.clone()),
                )
            })
            .when(is_track, |el| {
                el.child(field_row(
                    tr().tag_track_number.clone(),
                    Input::new(&self.track_number)
                        .small()
                        .disabled(!track_fields),
                    muted,
                ))
                .child(field_row(
                    tr().tag_disc_number.clone(),
                    Input::new(&self.disc_number)
                        .small()
                        .disabled(!track_fields),
                    muted,
                ))
            })
            .child(self.genre_section(muted, border, cx))
            .when(
                is_track && (!self.raw_tags.is_empty() || self.can_add_custom()),
                |el| el.child(self.raw_section(muted, border, cx)),
            )
    }

    fn genre_section(
        &self,
        muted: gpui::Hsla,
        border: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let editable = !self.read_only();
        let accent = Colors::accent(cx);
        let chips = self
            .genres
            .iter()
            .enumerate()
            .map(|(ix, genre)| {
                h_flex()
                    .gap_1()
                    .items_center()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .border_1()
                    .border_color(border)
                    .child(div().text_xs().child(genre.clone()))
                    .when(editable, |el| {
                        el.child(
                            chip_icon_button(
                                ElementId::NamedInteger("genre-remove".into(), ix as u64),
                                "icons/s1-x.svg",
                                muted,
                                accent,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if ix < this.genres.len() {
                                        this.genres.remove(ix);
                                        cx.notify();
                                    }
                                },
                            )),
                        )
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(tr().tag_genres.clone()),
            )
            .when(!chips.is_empty(), |el| {
                el.child(h_flex().flex_wrap().gap_1p5().children(chips))
            })
            .when(editable, |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(Input::new(&self.genre_input).small().cleanable(false)),
                        )
                        .child(
                            action_icon_button(
                                ElementId::Name("genre-add".into()),
                                "icons/s1-plus.svg",
                                muted,
                                accent,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.commit_genre(window, cx);
                                },
                            )),
                        ),
                )
            })
    }

    fn raw_section(
        &self,
        muted: gpui::Hsla,
        border: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let editable = !self.read_only();
        let accent = Colors::accent(cx);

        v_flex()
            .gap_1p5()
            .pt_2()
            .border_t_1()
            .border_color(border)
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(tr().tag_all_tags.clone()),
            )
            .children(
                self.raw_tags
                    .iter()
                    .enumerate()
                    .map(|(ix, (key, value))| {
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .w(px(LABEL_WIDTH))
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(key.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_xs()
                                    .text_ellipsis()
                                    .child(value.clone()),
                            )
                            .when(editable, |el| {
                                el.child(
                                    action_icon_button(
                                        ElementId::NamedInteger("raw-tag-remove".into(), ix as u64),
                                        "icons/s1-x.svg",
                                        muted,
                                        accent,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.remove_raw_tag(ix);
                                            cx.notify();
                                        },
                                    )),
                                )
                            })
                    })
                    .collect::<Vec<_>>(),
            )
            .when(self.can_add_custom(), |el| {
                el.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .w(px(LABEL_WIDTH))
                                .flex_shrink_0()
                                .child(Input::new(&self.custom_key).small().cleanable(false)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(Input::new(&self.custom_value).small().cleanable(false)),
                        )
                        .child(
                            action_icon_button(
                                ElementId::Name("raw-tag-add".into()),
                                "icons/s1-plus.svg",
                                muted,
                                accent,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.commit_custom(window, cx);
                                },
                            )),
                        ),
                )
            })
    }

    fn remove_raw_tag(&mut self, ix: usize) {
        if ix < self.raw_tags.len() {
            self.raw_tags.remove(ix);
        }
    }
}

/// Round icon button. Needs an explicit size: an unsized `div` around an `svg`
/// collapses to a zero hitbox, so the icon renders but nothing is clickable.
fn icon_button(
    id: ElementId,
    icon: &'static str,
    icon_color: gpui::Hsla,
    hover_bg: gpui::Hsla,
    button_size: f32,
    icon_size: f32,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .size(rems(button_size / 16.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            svg()
                .path(icon)
                .size(rems(icon_size / 16.))
                .text_color(icon_color),
        )
}

/// Sits inside a genre chip, so it stays smaller than the standalone row controls.
fn chip_icon_button(
    id: ElementId,
    icon: &'static str,
    icon_color: gpui::Hsla,
    hover_bg: gpui::Hsla,
) -> gpui::Stateful<gpui::Div> {
    icon_button(
        id,
        icon,
        icon_color,
        hover_bg,
        CHIP_BUTTON_SIZE,
        CHIP_ICON_SIZE,
    )
}

fn action_icon_button(
    id: ElementId,
    icon: &'static str,
    icon_color: gpui::Hsla,
    hover_bg: gpui::Hsla,
) -> gpui::Stateful<gpui::Div> {
    icon_button(
        id,
        icon,
        icon_color,
        hover_bg,
        ACTION_BUTTON_SIZE,
        ACTION_ICON_SIZE,
    )
}

/// Bytes as the user thinks of them. Only ever shown for a file they just picked, so
/// megabytes is the useful unit and one decimal is plenty.
fn format_size(bytes: usize) -> String {
    const MB: f64 = 1024. * 1024.;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{} KB", (bytes as f64 / 1024.).max(1.).round() as usize)
    }
}

/// Decode picked bytes for the preview off the render thread. Mirrors how
/// `cover_mode_view` builds its full-size image.
async fn decode_preview(bytes: &[u8], cx: &mut gpui::AsyncApp) -> Option<Arc<RenderImage>> {
    let format = crate::cover_mode_view::sniff_image_format(bytes)?;
    let owned = bytes.to_vec();
    let renderer = cx.update(|cx| cx.svg_renderer()).ok()?;
    cx.background_executor()
        .spawn(async move {
            Image::from_bytes(format, owned)
                .to_image_data(renderer)
                .ok()
        })
        .await
}

fn field_row(label: SharedString, input: Input, muted: gpui::Hsla) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_shrink_0()
                .text_sm()
                .text_color(muted)
                .child(label),
        )
        .child(div().flex_1().min_w(px(0.)).child(input))
}
