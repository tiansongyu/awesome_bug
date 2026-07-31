//! Generic sprite-rig planning with no SDL dependency.

use std::error::Error;
use std::fmt;

use crate::contract::{BodyState, Pose, Rect, SourceRect};
use crate::math::{Vec2, rotate_local};
use crate::species::{PartDefinition, Species, VisualDefinition};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DrawPass {
    Shadow,
    Sprite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColorMod {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawCommand {
    /// Stable manifest index; renderers use it only for diagnostics.
    pub part_index: usize,
    pub layer: i32,
    pub pass: DrawPass,
    pub source: SourceRect,
    pub destination: Rect,
    /// Pivot relative to `destination` in destination pixels.
    pub pivot: Vec2,
    /// Clockwise screen-space rotation in radians.
    pub rotation: f32,
    pub color: ColorMod,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RigPlan {
    pub body_center: Vec2,
    pub sprite_scale: f32,
    pub commands: Vec<DrawCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RigError {
    AtlasDimensions {
        actual_width: i32,
        actual_height: i32,
        required_width: i32,
        required_height: i32,
    },
    PosePartCount {
        actual: usize,
        required: usize,
    },
    InvalidBody,
    InvalidCanvasCenter,
    InvalidPose {
        part_index: Option<usize>,
    },
    InvalidSpeciesGeometry {
        part_index: Option<usize>,
    },
}

impl fmt::Display for RigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtlasDimensions {
                actual_width,
                actual_height,
                required_width,
                required_height,
            } => write!(
                formatter,
                "atlas dimensions are {actual_width}x{actual_height}, manifest requires \
                 {required_width}x{required_height}"
            ),
            Self::PosePartCount { actual, required } => {
                write!(
                    formatter,
                    "pose contains {actual} parts, manifest requires {required}"
                )
            }
            Self::InvalidBody => write!(formatter, "body state is not finite and positive"),
            Self::InvalidCanvasCenter => write!(formatter, "canvas center is not finite"),
            Self::InvalidPose { part_index: None } => {
                write!(formatter, "body pose contains a non-finite value")
            }
            Self::InvalidPose {
                part_index: Some(index),
            } => write!(formatter, "pose part {index} contains a non-finite value"),
            Self::InvalidSpeciesGeometry { part_index: None } => {
                write!(
                    formatter,
                    "species reference length is not finite and positive"
                )
            }
            Self::InvalidSpeciesGeometry {
                part_index: Some(index),
            } => write!(
                formatter,
                "species part {index} contains invalid sprite geometry"
            ),
        }
    }
}

impl Error for RigError {}

/// Precompiles stable manifest layer order and emits renderer-neutral commands.
#[derive(Clone, Debug)]
pub struct RigPlanner {
    atlas_width: i32,
    atlas_height: i32,
    reference_length: f32,
    visual: VisualDefinition,
    parts: Vec<PartDefinition>,
    draw_order: Vec<usize>,
}

impl RigPlanner {
    #[must_use]
    pub fn new(species: &Species) -> Self {
        let mut draw_order: Vec<usize> = (0..species.parts.len()).collect();
        draw_order.sort_by_key(|&index| species.parts[index].layer);
        Self {
            atlas_width: species.atlas.width,
            atlas_height: species.atlas.height,
            reference_length: species.atlas.reference_length,
            visual: species.visual,
            parts: species.parts.clone(),
            draw_order,
        }
    }

    pub fn ensure_atlas_dimensions(
        &self,
        actual_width: i32,
        actual_height: i32,
    ) -> Result<(), RigError> {
        if actual_width != self.atlas_width || actual_height != self.atlas_height {
            return Err(RigError::AtlasDimensions {
                actual_width,
                actual_height,
                required_width: self.atlas_width,
                required_height: self.atlas_height,
            });
        }
        Ok(())
    }

    pub fn plan(
        &self,
        pose: &Pose,
        body: BodyState,
        canvas_center: Vec2,
    ) -> Result<RigPlan, RigError> {
        self.validate(pose, body, canvas_center)?;

        let pose_heading = body.heading + pose.body_rotation;
        let body_center = canvas_center + rotate_local(pose.body_offset, pose_heading);
        let sprite_scale = body.length / self.reference_length;
        let pass_count = if self.visual.shadow_alpha == 0 { 1 } else { 2 };
        let mut commands = Vec::with_capacity(self.parts.len() * pass_count);

        if self.visual.shadow_alpha != 0 {
            self.append_pass(
                &mut commands,
                pose,
                body,
                body_center,
                sprite_scale,
                pose_heading,
                DrawPass::Shadow,
                self.visual.shadow_offset,
                ColorMod {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: self.visual.shadow_alpha,
                },
            );
        }
        self.append_pass(
            &mut commands,
            pose,
            body,
            body_center,
            sprite_scale,
            pose_heading,
            DrawPass::Sprite,
            Vec2::ZERO,
            ColorMod {
                red: self.visual.red,
                green: self.visual.green,
                blue: self.visual.blue,
                alpha: self.visual.alpha,
            },
        );

        Ok(RigPlan {
            body_center,
            sprite_scale,
            commands,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn append_pass(
        &self,
        commands: &mut Vec<DrawCommand>,
        pose: &Pose,
        body: BodyState,
        body_center: Vec2,
        sprite_scale: f32,
        pose_heading: f32,
        pass: DrawPass,
        screen_offset: Vec2,
        color: ColorMod,
    ) {
        for &index in &self.draw_order {
            let part = &self.parts[index];
            let part_pose = pose.parts[index];
            let local_joint = part.attachment * body.length + part_pose.joint_offset;
            let joint = body_center + rotate_local(local_joint, pose_heading) + screen_offset;
            let pivot = part.pivot * sprite_scale;
            commands.push(DrawCommand {
                part_index: index,
                layer: part.layer,
                pass,
                source: part.source,
                destination: Rect {
                    x: joint.x - pivot.x,
                    y: joint.y - pivot.y,
                    width: part.source.width as f32 * sprite_scale,
                    height: part.source.height as f32 * sprite_scale,
                },
                pivot,
                rotation: pose_heading + part_pose.rotation,
                color,
            });
        }
    }

    fn validate(&self, pose: &Pose, body: BodyState, canvas_center: Vec2) -> Result<(), RigError> {
        if pose.parts.len() != self.parts.len() {
            return Err(RigError::PosePartCount {
                actual: pose.parts.len(),
                required: self.parts.len(),
            });
        }
        if !body.position.is_finite()
            || !body.heading.is_finite()
            || !body.speed.is_finite()
            || !body.length.is_finite()
            || body.length <= 0.0
        {
            return Err(RigError::InvalidBody);
        }
        if !canvas_center.is_finite() {
            return Err(RigError::InvalidCanvasCenter);
        }
        if !pose.body_offset.is_finite() || !pose.body_rotation.is_finite() {
            return Err(RigError::InvalidPose { part_index: None });
        }
        if !self.reference_length.is_finite() || self.reference_length <= 0.0 {
            return Err(RigError::InvalidSpeciesGeometry { part_index: None });
        }

        for (index, (part, part_pose)) in self.parts.iter().zip(&pose.parts).enumerate() {
            if !part.pivot.is_finite()
                || !part.attachment.is_finite()
                || part.source.width <= 0
                || part.source.height <= 0
            {
                return Err(RigError::InvalidSpeciesGeometry {
                    part_index: Some(index),
                });
            }
            if !part_pose.rotation.is_finite() || !part_pose.joint_offset.is_finite() {
                return Err(RigError::InvalidPose {
                    part_index: Some(index),
                });
            }
        }
        Ok(())
    }
}
