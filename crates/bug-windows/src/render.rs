//! Strict PNG loading and renderer-safe SDL sprite-rig drawing.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek};
use std::path::{Path, PathBuf};

use bug_runtime::rig::{DrawCommand, DrawPass, RigPlan};
use png::{BitDepth, ColorType, Decoder, Limits, Transformations};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::{FPoint, FRect, Rect};
use sdl2::render::{BlendMode, SurfaceCanvas, Texture};
use sdl2::surface::Surface;

use crate::platform::layered_window::{LayeredWindow, RendererKey, RendererResources};

pub const MAX_PNG_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PNG_DIMENSION: u32 = 16_384;
pub const MAX_RGBA_BYTES: usize = 256 * 1024 * 1024;
pub const BAIT_OVERLAY_SIZE: u32 = 84;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// CPU-owned, normalized, straight-alpha RGBA8 image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    visible_bounds: PixelBounds,
}

impl RgbaImage {
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub const fn visible_bounds(&self) -> PixelBounds {
        self.visible_bounds
    }

    fn to_surface(&self) -> Result<Surface<'static>, RenderError> {
        let mut surface = Surface::new(self.width, self.height, PixelFormatEnum::RGBA32)
            .map_err(|message| RenderError::sdl("create atlas upload surface", message))?;
        let pitch = surface.pitch() as usize;
        let row_bytes = self.width as usize * 4;
        let Some(destination) = surface.without_lock_mut() else {
            return Err(RenderError::sdl(
                "write atlas upload surface",
                "new software surface unexpectedly requires locking",
            ));
        };
        if pitch < row_bytes || destination.len() < pitch * self.height as usize {
            return Err(RenderError::sdl(
                "write atlas upload surface",
                "SDL surface pitch or storage is smaller than expected",
            ));
        }
        for row in 0..self.height as usize {
            let source_start = row * row_bytes;
            let destination_start = row * pitch;
            destination[destination_start..destination_start + row_bytes]
                .copy_from_slice(&self.pixels[source_start..source_start + row_bytes]);
        }
        Ok(surface)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenderError {
    File {
        path: PathBuf,
        message: String,
    },
    Png(String),
    Limit(String),
    InvalidImage(String),
    InvalidPlan(String),
    RendererMismatch,
    Sdl {
        operation: &'static str,
        message: String,
    },
}

impl RenderError {
    fn sdl(operation: &'static str, message: impl Into<String>) -> Self {
        Self::Sdl {
            operation,
            message: message.into(),
        }
    }
}

impl Display for RenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { path, message } => {
                write!(formatter, "cannot load PNG {}: {message}", path.display())
            }
            Self::Png(message) => write!(formatter, "PNG decoding failed: {message}"),
            Self::Limit(message) => write!(formatter, "PNG limit exceeded: {message}"),
            Self::InvalidImage(message) => write!(formatter, "invalid PNG image: {message}"),
            Self::InvalidPlan(message) => write!(formatter, "invalid rig plan: {message}"),
            Self::RendererMismatch => {
                write!(
                    formatter,
                    "atlas texture does not belong to this SDL renderer"
                )
            }
            Self::Sdl { operation, message } => {
                write!(formatter, "{operation}: {message}")
            }
        }
    }
}

impl Error for RenderError {}

pub fn decode_png_path(path: impl AsRef<Path>) -> Result<RgbaImage, RenderError> {
    let path = path.as_ref();
    let metadata = std::fs::metadata(path).map_err(|error| RenderError::File {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(RenderError::File {
            path: path.to_path_buf(),
            message: "path is not a regular file".to_owned(),
        });
    }
    if metadata.len() > MAX_PNG_FILE_BYTES as u64 {
        return Err(RenderError::Limit(format!(
            "{} is larger than the {MAX_PNG_FILE_BYTES}-byte encoded-file limit",
            path.display()
        )));
    }
    let file = File::open(path).map_err(|error| RenderError::File {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    decode_png_reader(BufReader::new(file))
}

pub fn decode_png_bytes(bytes: &[u8]) -> Result<RgbaImage, RenderError> {
    if bytes.len() > MAX_PNG_FILE_BYTES {
        return Err(RenderError::Limit(format!(
            "encoded input is larger than {MAX_PNG_FILE_BYTES} bytes"
        )));
    }
    decode_png_reader(Cursor::new(bytes))
}

fn decode_png_reader<R>(reader: R) -> Result<RgbaImage, RenderError>
where
    R: BufRead + Seek,
{
    let mut decoder = Decoder::new_with_limits(
        reader,
        Limits {
            bytes: MAX_RGBA_BYTES,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    // ALPHA implies palette/low-bit expansion and also supplies an opaque
    // alpha channel when the source has none.  STRIP_16 makes every supported
    // input exactly eight bits per sample.
    decoder.set_transformations(Transformations::ALPHA | Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|error| RenderError::Png(error.to_string()))?;

    let (width, height, animated) = {
        let info = reader.info();
        (info.width, info.height, info.animation_control.is_some())
    };
    validate_png_dimensions(width, height)?;
    if animated {
        return Err(RenderError::InvalidImage(
            "animated PNG atlases are not supported".to_owned(),
        ));
    }
    let rgba_len = rgba_byte_len(width, height)?;
    let decoded_len = reader.output_buffer_size().ok_or_else(|| {
        RenderError::Limit("decoder output buffer size does not fit this process".to_owned())
    })?;
    if decoded_len > MAX_RGBA_BYTES {
        return Err(RenderError::Limit(format!(
            "decoder output requires {decoded_len} bytes; maximum is {MAX_RGBA_BYTES}"
        )));
    }

    let mut decoded = vec![0; decoded_len];
    let output = reader
        .next_frame(&mut decoded)
        .map_err(|error| RenderError::Png(error.to_string()))?;
    if output.width != width || output.height != height {
        return Err(RenderError::InvalidImage(
            "decoded frame dimensions differ from the PNG header".to_owned(),
        ));
    }
    if output.bit_depth != BitDepth::Eight {
        return Err(RenderError::InvalidImage(format!(
            "decoder produced unsupported {:?} samples",
            output.bit_depth
        )));
    }
    decoded.truncate(output.buffer_size());

    let pixels = match output.color_type {
        ColorType::Rgba => {
            if decoded.len() != rgba_len {
                return Err(RenderError::InvalidImage(
                    "RGBA output length does not match image dimensions".to_owned(),
                ));
            }
            decoded
        }
        ColorType::GrayscaleAlpha => {
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(2))
                .ok_or_else(|| {
                    RenderError::Limit("grayscale-alpha byte size overflows".to_owned())
                })?;
            if decoded.len() != expected {
                return Err(RenderError::InvalidImage(
                    "grayscale-alpha output length does not match image dimensions".to_owned(),
                ));
            }
            let mut rgba = Vec::with_capacity(rgba_len);
            for pixel in decoded.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
            rgba
        }
        other => {
            return Err(RenderError::InvalidImage(format!(
                "decoder did not normalize PNG to alpha-bearing 8-bit pixels ({other:?})"
            )));
        }
    };

    let visible_bounds = visible_pixel_bounds(width, height, &pixels)
        .ok_or_else(|| RenderError::InvalidImage("atlas is fully transparent".to_owned()))?;
    Ok(RgbaImage {
        width,
        height,
        pixels,
        visible_bounds,
    })
}

fn validate_png_dimensions(width: u32, height: u32) -> Result<(), RenderError> {
    if width == 0 || height == 0 {
        return Err(RenderError::InvalidImage(
            "width and height must be positive".to_owned(),
        ));
    }
    if width > MAX_PNG_DIMENSION || height > MAX_PNG_DIMENSION {
        return Err(RenderError::Limit(format!(
            "{width}x{height} exceeds the {MAX_PNG_DIMENSION}-pixel dimension limit"
        )));
    }
    let _ = rgba_byte_len(width, height)?;
    Ok(())
}

fn rgba_byte_len(width: u32, height: u32) -> Result<usize, RenderError> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| RenderError::Limit("RGBA byte size overflows".to_owned()))?;
    if bytes > MAX_RGBA_BYTES {
        return Err(RenderError::Limit(format!(
            "RGBA output requires {bytes} bytes; maximum is {MAX_RGBA_BYTES}"
        )));
    }
    Ok(bytes)
}

fn visible_pixel_bounds(width: u32, height: u32, rgba: &[u8]) -> Option<PixelBounds> {
    let mut minimum_x = width;
    let mut minimum_y = height;
    let mut maximum_x = 0;
    let mut maximum_y = 0;
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            let alpha = rgba[((y as usize * width as usize + x as usize) * 4) + 3];
            if alpha != 0 {
                minimum_x = minimum_x.min(x);
                minimum_y = minimum_y.min(y);
                maximum_x = maximum_x.max(x);
                maximum_y = maximum_y.max(y);
                found = true;
            }
        }
    }
    found.then_some(PixelBounds {
        x: minimum_x,
        y: minimum_y,
        width: maximum_x - minimum_x + 1,
        height: maximum_y - minimum_y + 1,
    })
}

struct RendererAtlas<'creator> {
    key: RendererKey,
    texture: Texture<'creator>,
}

/// One atlas uploaded once to each overlay's software renderer.
///
/// The lifetime ties every texture to the stable `RendererResources` slice
/// supplied by the application.  Declare resources before the session so
/// normal reverse drop order is `session -> resources -> windows -> SDL`.
pub struct RenderSession<'creator> {
    atlas_width: u32,
    atlas_height: u32,
    renderers: Vec<RendererAtlas<'creator>>,
}

impl<'creator> RenderSession<'creator> {
    pub fn new(
        resources: &'creator [RendererResources],
        atlas: &RgbaImage,
    ) -> Result<Self, RenderError> {
        if resources.is_empty() {
            return Err(RenderError::RendererMismatch);
        }

        let surface = atlas.to_surface()?;
        let mut renderers = Vec::with_capacity(resources.len());
        for resource in resources {
            let mut texture = resource
                .creator()
                .create_texture_from_surface(&surface)
                .map_err(|error| RenderError::sdl("upload atlas texture", error.to_string()))?;
            texture.set_blend_mode(BlendMode::Blend);
            renderers.push(RendererAtlas {
                key: resource.key(),
                texture,
            });
        }
        Ok(Self {
            atlas_width: atlas.width,
            atlas_height: atlas.height,
            renderers,
        })
    }

    #[must_use]
    pub fn renderer_count(&self) -> usize {
        self.renderers.len()
    }

    /// Clears one overlay and renders an already validated rig plan.  The
    /// caller submits the completed surface with `LayeredWindow::present_at`.
    pub fn render(
        &mut self,
        window: &mut LayeredWindow,
        plan: &RigPlan,
    ) -> Result<(), RenderError> {
        validate_plan(plan, self.atlas_width, self.atlas_height)?;
        let key = window.renderer_key();
        let atlas = self
            .renderers
            .iter_mut()
            .find(|atlas| atlas.key == key)
            .ok_or(RenderError::RendererMismatch)?;
        draw_plan(window.canvas_mut(), &mut atlas.texture, plan)
    }
}

fn validate_plan(plan: &RigPlan, atlas_width: u32, atlas_height: u32) -> Result<(), RenderError> {
    if !plan.body_center.is_finite() || !plan.sprite_scale.is_finite() || plan.sprite_scale <= 0.0 {
        return Err(RenderError::InvalidPlan(
            "body center and sprite scale must be finite and positive".to_owned(),
        ));
    }

    let mut previous: Option<(DrawPass, i32)> = None;
    for command in &plan.commands {
        validate_command(command, atlas_width, atlas_height)?;
        if let Some((previous_pass, previous_layer)) = previous
            && (command.pass < previous_pass
                || (command.pass == previous_pass && command.layer < previous_layer))
        {
            return Err(RenderError::InvalidPlan(
                "commands must contain all shadows first and be layer-sorted per pass".to_owned(),
            ));
        }
        previous = Some((command.pass, command.layer));
    }
    Ok(())
}

fn validate_command(
    command: &DrawCommand,
    atlas_width: u32,
    atlas_height: u32,
) -> Result<(), RenderError> {
    let source = command.source;
    let source_right = i64::from(source.x) + i64::from(source.width);
    let source_bottom = i64::from(source.y) + i64::from(source.height);
    if source.x < 0
        || source.y < 0
        || source.width <= 0
        || source.height <= 0
        || source_right > i64::from(atlas_width)
        || source_bottom > i64::from(atlas_height)
    {
        return Err(RenderError::InvalidPlan(format!(
            "part {} source rectangle lies outside the atlas",
            command.part_index
        )));
    }
    if !command.destination.is_finite()
        || command.destination.width <= 0.0
        || command.destination.height <= 0.0
        || !command.pivot.is_finite()
        || !command.rotation.is_finite()
    {
        return Err(RenderError::InvalidPlan(format!(
            "part {} has non-finite or non-positive draw geometry",
            command.part_index
        )));
    }
    match command.pass {
        DrawPass::Shadow
            if command.color.red != 0 || command.color.green != 0 || command.color.blue != 0 =>
        {
            Err(RenderError::InvalidPlan(format!(
                "part {} shadow must be black",
                command.part_index
            )))
        }
        DrawPass::Sprite if command.color.alpha != 255 => Err(RenderError::InvalidPlan(format!(
            "part {} sprite alpha modulation must be 255",
            command.part_index
        ))),
        _ => Ok(()),
    }
}

fn draw_plan(
    canvas: &mut SurfaceCanvas<'static>,
    texture: &mut Texture<'_>,
    plan: &RigPlan,
) -> Result<(), RenderError> {
    clear_transparent(canvas);
    for command in &plan.commands {
        texture.set_color_mod(command.color.red, command.color.green, command.color.blue);
        // Sprite modulation is validated as 255.  Per-pixel atlas alpha still
        // antialiases silhouettes; there is no whole-body translucency.
        texture.set_alpha_mod(command.color.alpha);
        let source = Rect::new(
            command.source.x,
            command.source.y,
            command.source.width as u32,
            command.source.height as u32,
        );
        let destination = FRect::new(
            command.destination.x,
            command.destination.y,
            command.destination.width,
            command.destination.height,
        );
        let pivot = FPoint::new(command.pivot.x, command.pivot.y);
        canvas
            .copy_ex_f(
                texture,
                source,
                destination,
                f64::from(command.rotation.to_degrees()),
                pivot,
                false,
                false,
            )
            .map_err(|message| RenderError::sdl("draw rig part", message))?;
    }
    Ok(())
}

fn clear_transparent(canvas: &mut SurfaceCanvas<'static>) {
    canvas.set_blend_mode(BlendMode::None);
    canvas.set_draw_color(Color::RGBA(0, 0, 0, 0));
    canvas.clear();
    canvas.set_blend_mode(BlendMode::Blend);
}

/// Draws the existing 84x84 food bait without a texture.  All food pixels are
/// fully opaque; the transparent background remains click-through.
pub fn render_bait(window: &mut LayeredWindow) -> Result<(), RenderError> {
    if window.dimensions() != (BAIT_OVERLAY_SIZE, BAIT_OVERLAY_SIZE) {
        return Err(RenderError::InvalidPlan(format!(
            "bait overlay must be {BAIT_OVERLAY_SIZE}x{BAIT_OVERLAY_SIZE}"
        )));
    }
    let canvas = window.canvas_mut();
    clear_transparent(canvas);
    let center = BAIT_OVERLAY_SIZE as f32 * 0.5;
    for y in -17..=17 {
        let normalized_y = y as f32 / 17.0;
        let width = (1.0 - normalized_y * normalized_y).max(0.0).sqrt()
            * (22.0 + (y as f32 * 0.7).sin() * 2.0);
        let edge = !(-12..=13).contains(&y);
        canvas.set_draw_color(if edge {
            Color::RGBA(116, 72, 34, 255)
        } else {
            Color::RGBA(194, 132, 65, 255)
        });
        canvas
            .draw_fline(
                FPoint::new(center - width, center + y as f32),
                FPoint::new(center + width, center + y as f32),
            )
            .map_err(|message| RenderError::sdl("draw food body", message))?;
    }

    canvas.set_draw_color(Color::RGBA(171, 106, 47, 255));
    for crumb in [
        FRect::new(center - 30.0, center + 18.0, 6.0, 5.0),
        FRect::new(center + 25.0, center + 11.0, 5.0, 5.0),
        FRect::new(center + 18.0, center - 27.0, 4.0, 4.0),
    ] {
        canvas
            .fill_frect(crumb)
            .map_err(|message| RenderError::sdl("draw food crumb", message))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use bug_runtime::contract::{Rect as RuntimeRect, SourceRect};
    use bug_runtime::math::Vec2;
    use bug_runtime::rig::{ColorMod, DrawCommand};
    use png::{BitDepth, ColorType, Encoder};

    use super::*;

    fn encoded_rgba(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        {
            let mut encoder = Encoder::new(&mut output, width, height);
            encoder.set_color(ColorType::Rgba);
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().expect("test PNG header");
            writer.write_image_data(pixels).expect("test PNG pixels");
        }
        output
    }

    #[test]
    fn png_is_normalized_and_fully_transparent_input_is_rejected() {
        let encoded = encoded_rgba(2, 1, &[10, 20, 30, 0, 40, 50, 60, 255]);
        let image = decode_png_bytes(&encoded).expect("valid PNG");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(image.pixels(), &[10, 20, 30, 0, 40, 50, 60, 255]);
        assert_eq!(
            image.visible_bounds(),
            PixelBounds {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            }
        );

        let transparent = encoded_rgba(1, 1, &[1, 2, 3, 0]);
        assert!(matches!(
            decode_png_bytes(&transparent),
            Err(RenderError::InvalidImage(message)) if message.contains("fully transparent")
        ));
    }

    #[test]
    fn software_blend_keeps_background_clear_body_opaque_and_edges_premultiplied() {
        let atlas = RgbaImage {
            width: 2,
            height: 1,
            pixels: vec![200, 100, 50, 255, 200, 100, 50, 128],
            visible_bounds: PixelBounds {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
        };
        let surface = Surface::new(4, 1, PixelFormatEnum::ARGB8888).expect("ARGB test surface");
        let mut canvas = surface.into_canvas().expect("software test renderer");
        let creator = canvas.texture_creator();
        let upload = atlas.to_surface().expect("upload surface");
        let mut texture = creator
            .create_texture_from_surface(&upload)
            .expect("test texture");
        texture.set_blend_mode(BlendMode::Blend);
        let plan = RigPlan {
            body_center: Vec2::new(2.0, 0.5),
            sprite_scale: 1.0,
            commands: vec![DrawCommand {
                part_index: 0,
                layer: 0,
                pass: DrawPass::Sprite,
                source: SourceRect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                destination: RuntimeRect {
                    x: 1.0,
                    y: 0.0,
                    width: 2.0,
                    height: 1.0,
                },
                pivot: Vec2::ZERO,
                rotation: 0.0,
                color: ColorMod {
                    red: 255,
                    green: 255,
                    blue: 255,
                    alpha: 255,
                },
            }],
        };
        validate_plan(&plan, atlas.width, atlas.height).expect("valid plan");
        draw_plan(&mut canvas, &mut texture, &plan).expect("draw plan");
        canvas.present();

        let pixels = canvas.surface().without_lock().expect("software pixels");
        // ARGB8888 is stored as BGRA bytes on little-endian Windows.
        assert_eq!(&pixels[0..4], &[0, 0, 0, 0]);
        assert_eq!(pixels[7], 255);
        assert_eq!(pixels[11], 128);
        assert!(pixels[8] <= pixels[11]);
        assert!(pixels[9] <= pixels[11]);
        assert!(pixels[10] <= pixels[11]);
    }

    #[test]
    fn translucent_sprite_modulation_is_rejected() {
        let plan = RigPlan {
            body_center: Vec2::ZERO,
            sprite_scale: 1.0,
            commands: vec![DrawCommand {
                part_index: 0,
                layer: 0,
                pass: DrawPass::Sprite,
                source: SourceRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                destination: RuntimeRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                pivot: Vec2::ZERO,
                rotation: 0.0,
                color: ColorMod {
                    red: 255,
                    green: 255,
                    blue: 255,
                    alpha: 254,
                },
            }],
        };
        assert!(matches!(
            validate_plan(&plan, 1, 1),
            Err(RenderError::InvalidPlan(message)) if message.contains("must be 255")
        ));
    }
}
