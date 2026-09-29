use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::plan::Skip;

const COVER_STEM: &str = "cover";
const TEMP_SUFFIX: &str = "pawse-tmp";
const MIN_SIDE: u32 = 200;

pub fn check(folder: &Path) -> Result<(), Skip> {
    let entries = std::fs::read_dir(folder).map_err(|e| io_skip(&e))?;
    let mut images: Vec<(String, u64)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !music_indexer::metadata::is_cover_image_name(&name) {
            continue;
        }
        let is_cover = name
            .rsplit_once('.')
            .is_some_and(|(stem, _)| stem.eq_ignore_ascii_case(COVER_STEM));
        if is_cover {
            return Err(Skip::CoverExists(name));
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        images.push((name, size));
    }
    let named: Vec<(&str, u64)> = images.iter().map(|(n, s)| (n.as_str(), *s)).collect();
    match music_indexer::metadata::best_cover_name(&named) {
        Some(ix) => Err(Skip::ImageExists(images[ix].0.clone())),
        None => Ok(()),
    }
}

pub fn validate(bytes: &[u8]) -> Result<&'static str, Skip> {
    let extension = cover_search::image_extension(bytes).ok_or(Skip::BadImage)?;
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .ok_or(Skip::BadImage)?;
    if width.min(height) < MIN_SIDE {
        return Err(Skip::BadImage);
    }
    Ok(extension)
}

pub fn write(folder: &Path, bytes: &[u8], extension: &str) -> Result<PathBuf, Skip> {
    check(folder)?;
    let target = folder.join(format!("{COVER_STEM}.{extension}"));
    let temp = folder.join(format!(".{COVER_STEM}.{extension}.{TEMP_SUFFIX}"));
    write_new(&temp, bytes)?;
    let placed = match std::fs::hard_link(&temp, &target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(existing(&target)),
        Err(_) if target.exists() => Err(existing(&target)),
        Err(_) => std::fs::rename(&temp, &target).map_err(|e| io_skip(&e)),
    };
    let _ = std::fs::remove_file(&temp);
    placed.map(|()| target)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Skip> {
    let open = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    };
    let mut file = match open() {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(path);
            open()
        }
        other => other,
    }
    .map_err(|e| io_skip(&e))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    written.map_err(|e| {
        let _ = std::fs::remove_file(path);
        io_skip(&e)
    })
}

fn existing(target: &Path) -> Skip {
    Skip::CoverExists(
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

fn io_skip(e: &std::io::Error) -> Skip {
    match e.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => Skip::ReadOnly,
        _ => Skip::WriteFailed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(side: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(side, side, image::Rgb([200, 40, 40]));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn writes_cover_jpg_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("01.flac"), b"x").unwrap();
        let bytes = jpeg(300);
        let written = write(dir.path(), &bytes, validate(&bytes).unwrap()).unwrap();
        assert_eq!(written, dir.path().join("cover.jpg"));
        assert_eq!(std::fs::read(&written).unwrap(), bytes);
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn never_overwrites_an_existing_cover() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cover.PNG"), b"mine").unwrap();
        let bytes = jpeg(300);
        assert_eq!(
            write(dir.path(), &bytes, "jpg"),
            Err(Skip::CoverExists("Cover.PNG".to_string()))
        );
        assert_eq!(
            std::fs::read(dir.path().join("Cover.PNG")).unwrap(),
            b"mine"
        );
        assert!(!dir.path().join("cover.jpg").exists());
    }

    #[test]
    fn an_image_the_scan_would_pick_blocks_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("folder.jpg"), b"x").unwrap();
        assert_eq!(
            check(dir.path()),
            Err(Skip::ImageExists("folder.jpg".to_string()))
        );
    }

    #[test]
    fn back_scans_alone_do_not_block_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("back.jpg"), b"x").unwrap();
        std::fs::write(dir.path().join("cd.png"), b"x").unwrap();
        assert_eq!(check(dir.path()), Ok(()));
    }

    #[test]
    fn stale_temp_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".cover.jpg.pawse-tmp"), b"old").unwrap();
        let bytes = jpeg(300);
        write(dir.path(), &bytes, "jpg").unwrap();
        assert_eq!(std::fs::read(dir.path().join("cover.jpg")).unwrap(), bytes);
        assert!(!dir.path().join(".cover.jpg.pawse-tmp").exists());
    }

    #[test]
    fn tiny_or_foreign_images_are_rejected() {
        assert_eq!(validate(&jpeg(64)), Err(Skip::BadImage));
        assert_eq!(validate(b"<html>not an image</html>"), Err(Skip::BadImage));
    }

    #[cfg(unix)]
    #[test]
    fn read_only_folder_is_reported_as_such() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = write(dir.path(), &jpeg(300), "jpg");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(result, Err(Skip::ReadOnly));
    }
}
