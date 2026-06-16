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
    let mut pawse_items = vec![MenuItem::action(s.rescan_library.clone(), Rescan)];
    if updater::is_supported() {
        pawse_items.push(MenuItem::action(
            s.check_for_updates.clone(),
            updater::CheckForUpdates,
        ));
    }
    pawse_items.push(MenuItem::separator());
    #[cfg(target_os = "macos")]
    {
        pawse_items.push(MenuItem::os_submenu(
            "Services",
            gpui::SystemMenuType::Services,
        ));
        pawse_items.push(MenuItem::separator());
        pawse_items.push(MenuItem::action(s.hide_pawse.clone(), Hide));
        pawse_items.push(MenuItem::action(s.hide_others.clone(), HideOthers));
        pawse_items.push(MenuItem::action(s.show_all.clone(), ShowAll));
        pawse_items.push(MenuItem::separator());
    }
    pawse_items.push(MenuItem::action(s.quit_pawse.clone(), Quit));
    vec![
        Menu {
            name: "Pawse".into(),
            items: pawse_items,
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
