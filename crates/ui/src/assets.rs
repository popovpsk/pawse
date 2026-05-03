use gpui::AssetSource;
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "icons/**/*"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Self::get(path)
            .map(|f| Some(f.data))
            .ok_or_else(|| anyhow::anyhow!("asset not found: {path}"))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(Self::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| p.into())
            .collect())
    }
}
