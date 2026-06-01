use gpui::{App, Menu, MenuItem, actions};

use crate::localization::tr;

actions!(
    pawse,
    [
        Rescan,
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        OpenRepository,
    ]
);

pub(crate) const REPOSITORY_URL: &str = "https://github.com/popovpsk/pawse/";

pub fn app_menus() -> Vec<Menu> {
    let s = tr();
    vec![
        Menu {
            name: "Pawse".into(),
            items: vec![
                MenuItem::action(s.rescan_library.clone(), Rescan),
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                #[cfg(target_os = "macos")]
                MenuItem::separator(),
                #[cfg(target_os = "macos")]
                MenuItem::action(s.hide_pawse.clone(), Hide),
                #[cfg(target_os = "macos")]
                MenuItem::action(s.hide_others.clone(), HideOthers),
                #[cfg(target_os = "macos")]
                MenuItem::action(s.show_all.clone(), ShowAll),
                #[cfg(target_os = "macos")]
                MenuItem::separator(),
                MenuItem::action(s.quit_pawse.clone(), Quit),
            ],
        },
        #[cfg(target_os = "macos")]
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action(s.minimize.clone(), Minimize),
                MenuItem::action(s.zoom.clone(), Zoom),
            ],
        },
        Menu {
            name: s.menu_help.clone(),
            items: vec![MenuItem::action(s.repository.clone(), OpenRepository)],
        },
    ]
}

pub fn set_menus(cx: &App) {
    cx.set_menus(app_menus());
}
