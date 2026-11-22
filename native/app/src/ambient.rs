//! Port of `Ambient.js`: a tiny blurred, saturated copy of the artwork, upscaled and blurred again
//! to stand in for the CSS `blur(90px)`.

use image::imageops::{blur, resize, FilterType};
use image::RgbaImage;

const SMALL: u32 = 48;
const DRAWN: u32 = 60;
const OFFSET: u32 = 6;
const LARGE: u32 = 256;
const SATURATION: f32 = 1.6;

fn saturate(image: &mut RgbaImage, s: f32) {
    for px in image.pixels_mut() {
        let [r, g, b, a] = px.0;
        let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
        let m = [
            0.213 + 0.787 * s,
            0.715 - 0.715 * s,
            0.072 - 0.072 * s,
            0.213 - 0.213 * s,
            0.715 + 0.285 * s,
            0.072 - 0.072 * s,
            0.213 - 0.213 * s,
            0.715 - 0.715 * s,
            0.072 + 0.928 * s,
        ];
        let clamp = |v: f32| v.clamp(0.0, 255.0).round() as u8;
        px.0 = [
            clamp(m[0] * r + m[1] * g + m[2] * b),
            clamp(m[3] * r + m[4] * g + m[5] * b),
            clamp(m[6] * r + m[7] * g + m[8] * b),
            a,
        ];
    }
}

pub fn render(art: &RgbaImage) -> RgbaImage {
    let drawn = resize(art, DRAWN, DRAWN, FilterType::Triangle);
    let mut small = image::imageops::crop_imm(&drawn, OFFSET, OFFSET, SMALL, SMALL).to_image();
    small = blur(&small, 3.0);
    saturate(&mut small, SATURATION);
    let large = resize(&small, LARGE, LARGE, FilterType::CatmullRom);
    blur(&large, 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_a_blurred_square_of_the_artwork_colour() {
        let art = RgbaImage::from_pixel(200, 200, image::Rgba([200, 40, 60, 255]));
        let out = render(&art);
        assert_eq!(out.dimensions(), (LARGE, LARGE));
        let px = out.get_pixel(128, 128).0;
        assert!(px[0] > px[1] && px[0] > px[2], "stays red: {px:?}");
        assert!(
            px[0] >= 200,
            "saturation boosts the dominant channel: {px:?}"
        );
    }

    #[test]
    fn saturation_leaves_grey_alone() {
        let mut img = RgbaImage::from_pixel(2, 2, image::Rgba([100, 100, 100, 255]));
        saturate(&mut img, 1.6);
        assert_eq!(img.get_pixel(0, 0).0, [100, 100, 100, 255]);
    }
}
