use gpui::{
    Anchor, App, Div, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, svg,
};
use gpui_component::{
    FocusableExt, Icon, Selectable, Sizable,
    button::{Button, ButtonGroup, ButtonVariants},
    h_flex,
    popover::Popover,
    switch::Switch,
    v_flex,
};
use ui_resources::i18n::view_menu_strings;

use crate::cover_backdrop::{popover_bg, veil_factor};
use crate::settings_store::{
    AlbumsArtistDisplay, AlbumsSort, ArtistsSort, LibraryLayout, SettingsStore, notify_save_error,
};
use crate::theme_colors::Colors;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMenuTab {
    Albums,
    Artists,
}

pub const TRIGGER_SIZE: f32 = 36.;
const MENU_WIDTH: f32 = 264.;
const ROW_HEIGHT: f32 = 30.;

pub fn view_menu(tab: ViewMenuTab, scale: f32) -> impl IntoElement {
    Popover::new("library-view-menu")
        .anchor(Anchor::TopLeft)
        .appearance(false)
        .trigger(
            Button::new("library-view-menu-trigger")
                .ghost()
                .compact()
                .rounded_full()
                .w(px(TRIGGER_SIZE * scale))
                .h(px(TRIGGER_SIZE * scale))
                .icon(
                    Icon::default()
                        .path("icons/s1-view.svg")
                        .size(px(20. * scale)),
                )
                .tooltip(view_menu_strings().view_options.clone()),
        )
        .content(move |_, _, cx| menu(tab, scale, cx))
}

fn save(cx: &mut App, write: impl FnOnce(&mut SettingsStore) -> anyhow::Result<()>) {
    if let Err(e) = write(cx.global_mut::<SettingsStore>()) {
        notify_save_error(cx, e);
    }
}

#[derive(Clone, Copy)]
struct MenuColors {
    foreground: Hsla,
    muted_fg: Hsla,
    hover: Hsla,
    border: Hsla,
    chip_on: Hsla,
}

fn menu(tab: ViewMenuTab, scale: f32, cx: &App) -> impl IntoElement + use<> {
    let s = view_menu_strings();
    let settings = cx.global::<SettingsStore>();
    let colors = MenuColors {
        foreground: Colors::foreground(cx),
        muted_fg: Colors::muted_foreground(cx),
        hover: Colors::muted(cx),
        border: Colors::border(cx),
        chip_on: Colors::secondary(cx),
    };

    let layout = settings.albums_layout();
    let grouping = match tab {
        ViewMenuTab::Albums => {
            let label = if settings.albums_sort().0 == AlbumsSort::Year {
                s.group_by_decade.clone()
            } else {
                s.group_by_letter.clone()
            };
            Some((label, settings.albums_grouped()))
        }
        ViewMenuTab::Artists => (settings.artists_sort().0 == ArtistsSort::Name)
            .then(|| (s.group_by_letter.clone(), settings.artists_grouped())),
    };

    v_flex()
        .id("library-view-menu-content")
        .w(px(MENU_WIDTH * scale))
        .p_1p5()
        .gap_0p5()
        .bg(popover_bg(Colors::popover(cx), veil_factor(cx)))
        .border_1()
        .border_color(colors.border)
        .rounded(px(8.))
        .shadow_md()
        .occlude()
        .when(tab == ViewMenuTab::Albums, |menu| {
            menu.child(layout_switch(layout))
        })
        .child(section_label(s.sort_by.clone(), colors))
        .map(|menu| match tab {
            ViewMenuTab::Albums => {
                let (current, desc) = settings.albums_sort();
                menu.children(
                    AlbumsSort::ALL
                        .into_iter()
                        .map(|sort| album_sort_row(sort, current, desc, colors)),
                )
            }
            ViewMenuTab::Artists => {
                let (current, desc) = settings.artists_sort();
                menu.children(
                    ArtistsSort::ALL
                        .into_iter()
                        .map(|sort| artist_sort_row(sort, current, desc, colors)),
                )
            }
        })
        .when_some(grouping, |menu, (label, grouped)| {
            menu.child(separator(colors))
                .child(group_row(tab, label, grouped, colors))
        })
        .when(tab == ViewMenuTab::Albums, |menu| {
            menu.child(separator(colors))
                .child(section_label(s.show.clone(), colors))
                .child(album_chips(settings, layout, colors))
                .when(
                    layout == LibraryLayout::List && settings.albums_show_artist(),
                    |menu| menu.child(artist_placement(settings, colors)),
                )
        })
}

fn layout_switch(layout: LibraryLayout) -> impl IntoElement {
    let s = view_menu_strings();
    h_flex().w_full().p_1().child(
        ButtonGroup::new("library-view-layout")
            .small()
            .w_full()
            .child(
                Button::new("library-view-grid")
                    .flex_1()
                    .icon(Icon::default().path("icons/s1-grid.svg"))
                    .label(s.grid.clone())
                    .selected(layout == LibraryLayout::Grid),
            )
            .child(
                Button::new("library-view-list")
                    .flex_1()
                    .icon(Icon::default().path("icons/s1-list.svg"))
                    .label(s.list.clone())
                    .selected(layout == LibraryLayout::List),
            )
            .on_click(move |clicks: &Vec<usize>, _, cx| {
                let Some(&ix) = clicks.first() else {
                    return;
                };
                let layout = if ix == 0 {
                    LibraryLayout::Grid
                } else {
                    LibraryLayout::List
                };
                save(cx, |s| s.set_albums_layout(layout));
            }),
    )
}

fn album_sort_row(
    sort: AlbumsSort,
    current: AlbumsSort,
    desc: bool,
    colors: MenuColors,
) -> impl IntoElement {
    let s = view_menu_strings();
    let (id, label, hints) = match sort {
        AlbumsSort::Artist => ("albums-sort-artist", &s.artist, (&s.a_to_z, &s.z_to_a)),
        AlbumsSort::Title => ("albums-sort-title", &s.title, (&s.a_to_z, &s.z_to_a)),
        AlbumsSort::Year => (
            "albums-sort-year",
            &s.year,
            (&s.oldest_first, &s.newest_first),
        ),
    };
    let active = sort == current;
    let hint = if desc { hints.1 } else { hints.0 };
    sort_row(id, label.clone(), hint.clone(), active, colors, move |cx| {
        save(cx, |s| s.set_albums_sort(sort, active && !desc));
    })
}

fn artist_sort_row(
    sort: ArtistsSort,
    current: ArtistsSort,
    desc: bool,
    colors: MenuColors,
) -> impl IntoElement {
    let s = view_menu_strings();
    let (id, label, hints) = match sort {
        ArtistsSort::Name => ("artists-sort-name", &s.name, (&s.a_to_z, &s.z_to_a)),
        ArtistsSort::Tracks => (
            "artists-sort-tracks",
            &s.track_count,
            (&s.fewest_first, &s.most_first),
        ),
    };
    let active = sort == current;
    let hint = if desc { hints.1 } else { hints.0 };
    sort_row(id, label.clone(), hint.clone(), active, colors, move |cx| {
        let next_desc = if active { !desc } else { sort.default_desc() };
        save(cx, |s| s.set_artists_sort(sort, next_desc));
    })
}

fn section_label(label: SharedString, colors: MenuColors) -> impl IntoElement {
    div()
        .px_2p5()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.muted_fg)
        .child(label)
}

fn separator(colors: MenuColors) -> impl IntoElement {
    div().mx_1().my_1().h(px(1.)).bg(colors.border)
}

fn sort_row(
    id: &'static str,
    label: SharedString,
    hint: SharedString,
    active: bool,
    colors: MenuColors,
    pick: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let hover = colors.hover;
    h_flex()
        .id(id)
        .min_h(px(ROW_HEIGHT))
        .px_2p5()
        .py_1()
        .gap_2()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(colors.foreground)
                .when(active, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(label),
        )
        .when(active, |row| {
            row.child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.muted_fg)
                    .child(hint)
                    .child(
                        svg()
                            .path("icons/s1-sort.svg")
                            .size(px(13.))
                            .text_color(colors.muted_fg),
                    ),
            )
        })
        .on_click(move |_, _, cx| pick(cx))
}

fn set_grouped(tab: ViewMenuTab, grouped: bool, cx: &mut App) {
    save(cx, |s| match tab {
        ViewMenuTab::Albums => s.set_albums_grouped(grouped),
        ViewMenuTab::Artists => s.set_artists_grouped(grouped),
    });
}

fn group_row(
    tab: ViewMenuTab,
    label: SharedString,
    grouped: bool,
    colors: MenuColors,
) -> impl IntoElement {
    let hover = colors.hover;
    h_flex()
        .id("library-view-group")
        .min_h(px(ROW_HEIGHT))
        .px_2p5()
        .py_1()
        .gap_2()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(colors.foreground)
                .child(label),
        )
        .child(
            Switch::new("library-view-group-switch")
                .small()
                .flex_shrink_0()
                .checked(grouped)
                .tab_stop(false)
                .focus_ring(false),
        )
        .on_click(move |_, _, cx| set_grouped(tab, !grouped, cx))
}

fn chip(
    id: &'static str,
    label: SharedString,
    on: bool,
    colors: MenuColors,
    toggle: impl Fn(&mut SettingsStore, bool) -> anyhow::Result<()> + 'static,
) -> impl IntoElement {
    let hover = colors.hover;
    h_flex()
        .id(id)
        .flex_shrink_0()
        .h(px(26.))
        .px_2p5()
        .gap_1()
        .rounded_full()
        .border_1()
        .text_xs()
        .cursor_pointer()
        .map(|chip| {
            if on {
                chip.bg(colors.chip_on)
                    .border_color(colors.chip_on)
                    .text_color(colors.foreground)
                    .child(
                        svg()
                            .path("icons/check.svg")
                            .size(px(11.))
                            .text_color(colors.foreground),
                    )
            } else {
                chip.border_color(colors.border)
                    .text_color(colors.muted_fg)
                    .hover(move |style| style.bg(hover))
            }
        })
        .child(label)
        .on_click(move |_, _, cx| save(cx, |s| toggle(s, !on)))
}

fn album_chips(settings: &SettingsStore, layout: LibraryLayout, colors: MenuColors) -> Div {
    let s = view_menu_strings();
    h_flex()
        .flex_wrap()
        .gap_1p5()
        .px_2()
        .pt_0p5()
        .pb_2()
        .child(chip(
            "albums-show-artist",
            s.artist.clone(),
            settings.albums_show_artist(),
            colors,
            SettingsStore::set_albums_show_artist,
        ))
        .child(chip(
            "albums-show-year",
            s.year.clone(),
            settings.albums_show_year(),
            colors,
            SettingsStore::set_albums_show_year,
        ))
        .when(layout == LibraryLayout::List, |chips| {
            chips.child(chip(
                "albums-show-genre",
                s.genre.clone(),
                settings.albums_show_genre(),
                colors,
                SettingsStore::set_albums_show_genre,
            ))
        })
}

fn artist_placement(settings: &SettingsStore, colors: MenuColors) -> impl IntoElement {
    let s = view_menu_strings();
    let column = settings.albums_artist_display() == AlbumsArtistDisplay::Column;
    h_flex()
        .px_2p5()
        .pb_2()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.muted_fg)
                .child(s.artist_goes.clone()),
        )
        .child(
            ButtonGroup::new("albums-artist-placement")
                .xsmall()
                .child(
                    Button::new("albums-artist-in-title")
                        .label(s.in_title.clone())
                        .selected(!column),
                )
                .child(
                    Button::new("albums-artist-column")
                        .label(s.column.clone())
                        .selected(column),
                )
                .on_click(|clicks: &Vec<usize>, _, cx| {
                    let Some(&ix) = clicks.first() else {
                        return;
                    };
                    let display = if ix == 1 {
                        AlbumsArtistDisplay::Column
                    } else {
                        AlbumsArtistDisplay::Inline
                    };
                    save(cx, |s| s.set_albums_artist_display(display));
                }),
        )
}
