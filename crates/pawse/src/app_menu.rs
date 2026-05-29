use gpui::{App, Menu, MenuItem, actions};

use crate::localization::tr;

actions!(pawse, [Rescan, Quit,]);

pub fn app_menus(cx: &App) -> Vec<Menu> {
    let s = tr(cx);
    vec![
        Menu {
            name: "Pawse".into(),
            items: vec![
                #[cfg(target_os = "macos")]
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                #[cfg(target_os = "macos")]
                MenuItem::separator(),
                MenuItem::action(s.quit_pawse.clone(), Quit),
            ],
        },
        Menu {
            name: s.menu_file.clone(),
            items: vec![MenuItem::action(s.rescan_library.clone(), Rescan)],
        },
    ]
}

/// (Re)install the application menus in the current language. Called at startup
/// and again whenever the UI language changes.
pub fn set_menus(cx: &App) {
    cx.set_menus(app_menus(cx));
}
