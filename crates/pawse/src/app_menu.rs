use gpui::{Menu, MenuItem, actions};

actions!(
    pawse,
    [
        Rescan,
        Quit,
    ]
);

pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Pawse".into(),
            items: vec![
                #[cfg(target_os = "macos")]
                MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
                #[cfg(target_os = "macos")]
                MenuItem::separator(),
                MenuItem::action("Quit Pawse", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Rescan Library...", Rescan)],
        },
    ]
}
