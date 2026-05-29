use std::path::PathBuf;

use gpui::App;
use gpui_component::theme::ThemeRegistry;

const BUNDLED_THEMES: &[(&str, &str)] = &[
    ("adventure.json", include_str!("../themes/adventure.json")),
    ("alduin.json", include_str!("../themes/alduin.json")),
    ("asciinema.json", include_str!("../themes/asciinema.json")),
    ("ayu.json", include_str!("../themes/ayu.json")),
    ("catppuccin.json", include_str!("../themes/catppuccin.json")),
    ("everforest.json", include_str!("../themes/everforest.json")),
    ("fahrenheit.json", include_str!("../themes/fahrenheit.json")),
    ("flexoki.json", include_str!("../themes/flexoki.json")),
    ("gruvbox.json", include_str!("../themes/gruvbox.json")),
    ("harper.json", include_str!("../themes/harper.json")),
    ("hybrid.json", include_str!("../themes/hybrid.json")),
    ("jellybeans.json", include_str!("../themes/jellybeans.json")),
    ("kibble.json", include_str!("../themes/kibble.json")),
    (
        "macos-classic.json",
        include_str!("../themes/macos-classic.json"),
    ),
    ("matrix.json", include_str!("../themes/matrix.json")),
    (
        "mellifluous.json",
        include_str!("../themes/mellifluous.json"),
    ),
    ("molokai.json", include_str!("../themes/molokai.json")),
    ("solarized.json", include_str!("../themes/solarized.json")),
    ("spaceduck.json", include_str!("../themes/spaceduck.json")),
    ("tokyonight.json", include_str!("../themes/tokyonight.json")),
    ("twilight.json", include_str!("../themes/twilight.json")),
];

fn stage_bundled_themes() -> anyhow::Result<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("data dir unavailable"))?
        .join("pawse")
        .join("themes");
    std::fs::create_dir_all(&dir)?;
    for (name, body) in BUNDLED_THEMES {
        let path = dir.join(name);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(dir)
}

/// Write bundled theme JSON files to disk and register them with `ThemeRegistry`.
/// `on_loaded` is called after the initial load completes.
pub fn register_bundled_themes<F: Fn(&mut App) + 'static>(cx: &mut App, on_loaded: F) {
    match stage_bundled_themes() {
        Ok(dir) => {
            if let Err(e) = ThemeRegistry::watch_dir(dir, cx, on_loaded) {
                eprintln!("failed to register bundled themes: {e}");
            }
        }
        Err(e) => eprintln!("failed to stage bundled themes: {e}"),
    }
}
