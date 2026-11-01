//! Renders `assets/app-icon/codecraft.svg` into the PNG (window icon) and ICO (Windows
//! executable) next to it: `cargo run --example make_icon`.

use image::{ImageFormat, RgbaImage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/app-icon");
    let svg = std::fs::read(dir.join("codecraft.svg"))?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    let render = |size: u32| -> Result<RgbaImage, Box<dyn std::error::Error>> {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or("bad size")?;
        let scale = size as f32 / tree.size().width();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        // tiny-skia keeps premultiplied alpha; PNG wants it straight.
        let pixels = pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        RgbaImage::from_raw(size, size, pixels).ok_or_else(|| "bad buffer".into())
    };
    render(256)?.save_with_format(dir.join("codecraft-256.png"), ImageFormat::Png)?;
    // The ICO encoder takes one image; 256 px is what Explorer scales down from.
    render(256)?.save_with_format(dir.join("codecraft.ico"), ImageFormat::Ico)?;
    println!("wrote codecraft-256.png and codecraft.ico");
    Ok(())
}
