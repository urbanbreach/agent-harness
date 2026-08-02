//! Fail-closed zero-tolerance RGBA PNG compare for TUI reference parity (unit path).

use std::path::Path;

use image::{Rgba, RgbaImage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PixelMismatch {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) reference: [u8; 4],
    pub(crate) actual: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PixelCompareOutcome {
    ExactMatch {
        width: u32,
        height: u32,
        total_pixels: u64,
    },
    DimensionMismatch {
        reference: (u32, u32),
        actual: (u32, u32),
    },
    RgbaMismatch {
        width: u32,
        height: u32,
        total_pixels: u64,
        mismatch_count: u64,
        first: Vec<PixelMismatch>,
    },
}

impl PixelCompareOutcome {
    pub(crate) fn is_pass(&self) -> bool {
        matches!(self, Self::ExactMatch { .. })
    }
}

pub(crate) fn load_rgba_png(path: &Path) -> RgbaImage {
    image::open(path)
        .unwrap_or_else(|err| panic!("failed to open PNG {}: {err}", path.display()))
        .to_rgba8()
}

pub(crate) fn compare_rgba_images(
    reference: &RgbaImage,
    actual: &RgbaImage,
) -> PixelCompareOutcome {
    let (rw, rh) = reference.dimensions();
    let (aw, ah) = actual.dimensions();
    if rw != aw || rh != ah {
        return PixelCompareOutcome::DimensionMismatch {
            reference: (rw, rh),
            actual: (aw, ah),
        };
    }

    let total_pixels = u64::from(rw) * u64::from(rh);
    let mut mismatch_count = 0_u64;
    let mut first = Vec::new();

    for y in 0..rh {
        for x in 0..rw {
            let Rgba(r) = *reference.get_pixel(x, y);
            let Rgba(a) = *actual.get_pixel(x, y);
            if r != a {
                mismatch_count += 1;
                if first.len() < 32 {
                    first.push(PixelMismatch {
                        x,
                        y,
                        reference: r,
                        actual: a,
                    });
                }
            }
        }
    }

    if mismatch_count == 0 {
        PixelCompareOutcome::ExactMatch {
            width: rw,
            height: rh,
            total_pixels,
        }
    } else {
        PixelCompareOutcome::RgbaMismatch {
            width: rw,
            height: rh,
            total_pixels,
            mismatch_count,
            first,
        }
    }
}

pub(crate) fn compare_png_paths(reference: &Path, actual: &Path) -> PixelCompareOutcome {
    if !reference.is_file() {
        panic!("missing reference PNG: {}", reference.display());
    }
    if !actual.is_file() {
        panic!("missing actual PNG: {}", actual.display());
    }
    compare_rgba_images(&load_rgba_png(reference), &load_rgba_png(actual))
}

pub(crate) fn solid_image(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    for pixel in image.pixels_mut() {
        *pixel = Rgba(color);
    }
    image
}

pub(crate) fn write_temp_png(dir: &Path, name: &str, image: &RgbaImage) -> std::path::PathBuf {
    let path = dir.join(name);
    image
        .save(&path)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
    path
}
