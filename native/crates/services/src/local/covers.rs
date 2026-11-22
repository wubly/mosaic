use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::DynamicImage;

pub struct Cover {
    pub artwork: String,
    pub thumb: String,
}

fn write_jpeg(image: &DynamicImage, max_width: u32, quality: u8, path: &Path) -> Result<()> {
    let width = image.width().min(max_width);
    let height = ((u64::from(image.height()) * u64::from(width)) / u64::from(image.width().max(1)))
        .max(1) as u32;
    let resized = image
        .resize_exact(width, height, FilterType::Lanczos3)
        .to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut Cursor::new(&mut out), quality).encode_image(&resized)?;
    fs::write(path, out)?;
    Ok(())
}

/// Writes the 400 px and 120 px covers for a local track id (`local:<key>`).
pub fn save_cover(cover_dir: &Path, id: &str, data: &[u8]) -> Result<Cover> {
    let image = image::load_from_memory(data)?;
    fs::create_dir_all(cover_dir)?;
    let key = id.strip_prefix("local:").unwrap_or(id);
    let large = cover_dir.join(format!("{key}.jpg"));
    let small = cover_dir.join(format!("{key}-s.jpg"));
    write_jpeg(&image, 400, 82, &large)?;
    write_jpeg(&image, 120, 80, &small)?;
    Ok(Cover {
        artwork: large.to_string_lossy().into_owned(),
        thumb: small.to_string_lossy().into_owned(),
    })
}
