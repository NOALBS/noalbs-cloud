//! Where a source sits on screen, set from a phone without dragging: presets
//! (fit, fill or stretch to a box that's all or part of the screen, or the
//! source's own size, in a corner, on an edge or in the center), or an exact
//! box in canvas pixels, like OBS's Edit Transform.
//!
//! Positions are on the canvas (`GetVideoSettings`' base size), which every
//! scene shares. Inside a group they're measured from the group instead, so
//! presets are only exact for a group that itself fills the screen.

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    ClientTransport, ObsClient,
    error::Result,
    types::{GetSceneItemTransform, GetVideoSettings, RequestKind},
};

/// Largest position, size or crop accepted, in pixels.
pub const MAX_PIXELS: f64 = 16_384.0;

const ALIGN_LEFT: i64 = 1;
const ALIGN_RIGHT: i64 = 2;
const ALIGN_TOP: i64 = 4;
const ALIGN_BOTTOM: i64 = 8;

/// Where on the screen (or in its box) a source sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// OBS's alignment flags for it.
    fn alignment(self) -> i64 {
        let (horizontal, vertical) = self.sides();
        horizontal + vertical
    }

    fn sides(self) -> (i64, i64) {
        match self {
            Self::TopLeft => (ALIGN_LEFT, ALIGN_TOP),
            Self::Top => (0, ALIGN_TOP),
            Self::TopRight => (ALIGN_RIGHT, ALIGN_TOP),
            Self::Left => (ALIGN_LEFT, 0),
            Self::Center => (0, 0),
            Self::Right => (ALIGN_RIGHT, 0),
            Self::BottomLeft => (ALIGN_LEFT, ALIGN_BOTTOM),
            Self::Bottom => (0, ALIGN_BOTTOM),
            Self::BottomRight => (ALIGN_RIGHT, ALIGN_BOTTOM),
        }
    }

    /// The point of a `width` × `height` canvas it pins to, `margin` in from
    /// the edges it's on.
    fn point(self, width: f64, height: f64, margin: f64) -> (f64, f64) {
        let along = |side: i64, start: i64, end: i64, length: f64| {
            if side == start {
                margin
            } else if side == end {
                length - margin
            } else {
                length / 2.0
            }
        };
        let (horizontal, vertical) = self.sides();
        (
            along(horizontal, ALIGN_LEFT, ALIGN_RIGHT, width),
            along(vertical, ALIGN_TOP, ALIGN_BOTTOM, height),
        )
    }
}

/// How a preset sizes the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// As large as fits in the box, keeping its shape.
    Fit,
    /// Covers the box, keeping its shape; what sticks out is cut off.
    Fill,
    /// Exactly the box, out of shape if need be.
    Stretch,
    /// Its own size (the box is ignored).
    Original,
}

/// Pixels cut off each side of the source, before it's sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Crop {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

/// Where to put a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Placement {
    /// A box `size_percent` of the screen (100: all of it), pinned to
    /// `anchor`, `margin_percent` of the screen's height in from its edges
    /// (none for a full-screen box; `Original` ignores the size but not the margin). The source is sized to the box by `fit`. Its crop stays.
    Preset {
        fit: Fit,
        anchor: Anchor,
        size_percent: u32,
        margin_percent: u32,
    },
    /// A box at `x`, `y` (its top left), `width` × `height` canvas pixels; the
    /// source fits inside it, keeping its shape, after `crop`.
    Exact {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        crop: Crop,
    },
}

/// What's wrong with a placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("a preset's size is 5% to 100% of the screen")]
    Size,
    #[error("a preset's margin is 0% to 25% of the screen")]
    Margin,
    #[error("a box is 1 to 16384 pixels wide and high, within 16384 pixels of the screen")]
    Box,
    #[error("crop is at most 16384 pixels a side")]
    Crop,
}

impl Placement {
    pub fn check(&self) -> std::result::Result<(), Invalid> {
        match self {
            Self::Preset {
                size_percent,
                margin_percent,
                ..
            } => {
                if !(5..=100).contains(size_percent) {
                    return Err(Invalid::Size);
                }
                if *margin_percent > 25 {
                    return Err(Invalid::Margin);
                }
            }
            Self::Exact {
                x,
                y,
                width,
                height,
                crop,
            } => {
                let within = |n: f64| n.is_finite() && n.abs() <= MAX_PIXELS;
                let size = |n: f64| n.is_finite() && (1.0..=MAX_PIXELS).contains(&n);
                if !within(*x) || !within(*y) || !size(*width) || !size(*height) {
                    return Err(Invalid::Box);
                }
                let sides = [crop.left, crop.right, crop.top, crop.bottom];
                if sides.iter().any(|side| f64::from(*side) > MAX_PIXELS) {
                    return Err(Invalid::Crop);
                }
            }
        }
        Ok(())
    }

    /// The `SetSceneItemTransform` for it on a `width` × `height` canvas.
    pub fn transform(&self, canvas: (u32, u32)) -> Value {
        let (width, height) = (f64::from(canvas.0), f64::from(canvas.1));
        match *self {
            Self::Preset {
                fit,
                anchor,
                size_percent,
                margin_percent,
            } => {
                let share = f64::from(size_percent) / 100.0;
                // A full-screen box sits flush; a smaller one, or the source at its own size, in.
                let margin = if size_percent == 100 && fit != Fit::Original {
                    0.0
                } else {
                    height * f64::from(margin_percent) / 100.0
                };
                let (x, y) = anchor.point(width, height, margin);
                let alignment = anchor.alignment();
                match fit {
                    Fit::Original => json!({
                        "boundsType": "OBS_BOUNDS_NONE",
                        "scaleX": 1.0, "scaleY": 1.0,
                        "alignment": alignment, "positionX": x, "positionY": y,
                    }),
                    fit => {
                        let bounds = match fit {
                            Fit::Fill => "OBS_BOUNDS_SCALE_OUTER",
                            Fit::Stretch => "OBS_BOUNDS_STRETCH",
                            _ => "OBS_BOUNDS_SCALE_INNER",
                        };
                        json!({
                            "boundsType": bounds,
                            "boundsWidth": (width * share).max(1.0),
                            "boundsHeight": (height * share).max(1.0),
                            "boundsAlignment": alignment,
                            // A filled box shows only what's inside it.
                            "cropToBounds": fit == Fit::Fill,
                            "alignment": alignment, "positionX": x, "positionY": y,
                        })
                    }
                }
            }
            Self::Exact {
                x,
                y,
                width,
                height,
                crop,
            } => json!({
                "boundsType": "OBS_BOUNDS_SCALE_INNER",
                "boundsWidth": width, "boundsHeight": height,
                "boundsAlignment": 0,
                "cropToBounds": false,
                "alignment": ALIGN_LEFT + ALIGN_TOP, "positionX": x, "positionY": y,
                "cropLeft": crop.left, "cropRight": crop.right,
                "cropTop": crop.top, "cropBottom": crop.bottom,
            }),
        }
    }
}

/// Where a source is now: its box's top left and size in canvas pixels (the
/// bounds if it has some, else its size on screen), and its crop.
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentPlacement {
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub crop: Crop,
    /// The source's own size, before crop and scaling; 0 × 0 while OBS doesn't
    /// know it (nothing playing).
    pub source_width: f64,
    pub source_height: f64,
    /// Turned, in degrees: the box is then only roughly where it shows.
    pub rotation: f64,
}

/// A scene's (or group's) items and where they are.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub canvas: (u32, u32),
    /// Top first.
    pub items: Vec<LayoutItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutItem {
    pub item_id: i64,
    pub name: String,
    pub enabled: bool,
    pub placement: CurrentPlacement,
}

/// [`CurrentPlacement`] from a `GetSceneItemTransform`.
fn current(transform: &Value, canvas: (u32, u32)) -> CurrentPlacement {
    let number = |key: &str| transform[key].as_f64().unwrap_or(0.0);
    let pixels = |key: &str| u32::try_from(transform[key].as_u64().unwrap_or(0)).unwrap_or(0);
    let has_bounds = transform["boundsType"]
        .as_str()
        .is_some_and(|bounds| bounds != "OBS_BOUNDS_NONE");
    let (width, height) = if has_bounds {
        (number("boundsWidth"), number("boundsHeight"))
    } else {
        (number("width"), number("height"))
    };
    let alignment = transform["alignment"]
        .as_i64()
        .unwrap_or(ALIGN_LEFT + ALIGN_TOP);
    // The position is of the aligned point: back to the top left.
    let offset = |start: i64, end: i64, length: f64| {
        if alignment & start != 0 {
            0.0
        } else if alignment & end != 0 {
            length
        } else {
            length / 2.0
        }
    };
    CurrentPlacement {
        canvas_width: canvas.0,
        canvas_height: canvas.1,
        x: number("positionX") - offset(ALIGN_LEFT, ALIGN_RIGHT, width),
        y: number("positionY") - offset(ALIGN_TOP, ALIGN_BOTTOM, height),
        width,
        height,
        crop: Crop {
            left: pixels("cropLeft"),
            right: pixels("cropRight"),
            top: pixels("cropTop"),
            bottom: pixels("cropBottom"),
        },
        source_width: number("sourceWidth"),
        source_height: number("sourceHeight"),
        rotation: number("rotation"),
    }
}

impl<CT: ClientTransport> ObsClient<CT> {
    /// The canvas size, in pixels.
    pub async fn canvas(&self) -> Result<(u32, u32)> {
        let video: GetVideoSettings = self.send_request(RequestKind::GetVideoSettings).await?;
        Ok((video.base_width, video.base_height))
    }

    /// Where everything in `scene` (a group when `group`) is now, top first
    /// like OBS's Sources dock: for drawing them, and moving one among them.
    pub async fn layout(&self, scene: &str, group: bool) -> Result<Layout> {
        let items = self.scene_items(scene, group).await?;
        let canvas = self.canvas().await?;
        let transforms = join_all(items.iter().map(|item| {
            self.send_request::<GetSceneItemTransform>(RequestKind::GetSceneItemTransform {
                scene_name: scene,
                scene_item_id: item.scene_item_id,
            })
        }))
        .await;
        let mut laid_out = Vec::with_capacity(items.len());
        for (item, transform) in items.into_iter().zip(transforms).rev() {
            laid_out.push(LayoutItem {
                item_id: item.scene_item_id,
                name: item.source_name,
                enabled: item.scene_item_enabled,
                placement: current(&transform?.scene_item_transform, canvas),
            });
        }
        Ok(Layout {
            canvas,
            items: laid_out,
        })
    }

    /// Puts item `item_id` of `scene` where `placement` says. Check it first.
    /// Returns its source's name, `None` if the item isn't there (anymore).
    pub async fn place(
        &self,
        scene: &str,
        group: bool,
        item_id: i64,
        placement: &Placement,
    ) -> Result<Option<String>> {
        let items = self.scene_items(scene, group).await?;
        let Some(item) = items.into_iter().find(|item| item.scene_item_id == item_id) else {
            return Ok(None);
        };
        self.place_item(scene, item_id, placement).await?;
        Ok(Some(item.source_name))
    }

    /// [`Self::place`] for an item known to be there (just made, say).
    pub async fn place_item(&self, scene: &str, item_id: i64, placement: &Placement) -> Result<()> {
        let canvas = self.canvas().await?;
        self.command(RequestKind::SetSceneItemTransform {
            scene_name: scene,
            scene_item_id: item_id,
            scene_item_transform: placement.transform(canvas),
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::fake::{client, input};

    fn preset(fit: Fit, anchor: Anchor, size_percent: u32, margin_percent: u32) -> Placement {
        Placement::Preset {
            fit,
            anchor,
            size_percent,
            margin_percent,
        }
    }

    #[test]
    fn fit_to_screen_is_bounds_the_size_of_the_canvas() {
        let transform = preset(Fit::Fit, Anchor::Center, 100, 3).transform((1920, 1080));
        assert_eq!(
            transform,
            json!({
                "boundsType": "OBS_BOUNDS_SCALE_INNER", "boundsWidth": 1920.0, "boundsHeight": 1080.0,
                "boundsAlignment": 0, "cropToBounds": false,
                "alignment": 0, "positionX": 960.0, "positionY": 540.0,
            })
        );
        let fill = preset(Fit::Fill, Anchor::TopLeft, 100, 0).transform((1920, 1080));
        assert_eq!(fill["boundsType"], "OBS_BOUNDS_SCALE_OUTER");
        assert_eq!(fill["cropToBounds"], true);
        assert_eq!(
            (fill["positionX"].as_f64(), fill["positionY"].as_f64()),
            (Some(0.0), Some(0.0))
        );
    }

    #[test]
    fn smaller_boxes_sit_a_margin_in_from_their_corner() {
        // A quarter-size box in the bottom right, 3% of 1080 = 32.4 px in.
        let corner = preset(Fit::Fit, Anchor::BottomRight, 25, 3).transform((1920, 1080));
        assert_eq!(corner["boundsWidth"], 480.0);
        assert_eq!(corner["boundsHeight"], 270.0);
        assert_eq!(corner["alignment"], ALIGN_RIGHT + ALIGN_BOTTOM);
        assert_eq!(corner["boundsAlignment"], ALIGN_RIGHT + ALIGN_BOTTOM);
        assert!((corner["positionX"].as_f64().unwrap() - 1887.6).abs() < 1e-9);
        assert!((corner["positionY"].as_f64().unwrap() - 1047.6).abs() < 1e-9);

        let text = preset(Fit::Original, Anchor::BottomLeft, 50, 3).transform((1280, 720));
        assert_eq!(
            text,
            json!({
                "boundsType": "OBS_BOUNDS_NONE", "scaleX": 1.0, "scaleY": 1.0,
                "alignment": ALIGN_LEFT + ALIGN_BOTTOM, "positionX": 21.6, "positionY": 698.4,
            })
        );
    }

    #[test]
    fn exact_boxes_are_measured_from_their_top_left() {
        let crop = Crop {
            left: 10,
            right: 0,
            top: 0,
            bottom: 20,
        };
        let exact = Placement::Exact {
            x: 100.0,
            y: 50.0,
            width: 640.0,
            height: 360.0,
            crop,
        };
        let transform = exact.transform((1920, 1080));
        assert_eq!(transform["alignment"], 5);
        assert_eq!(transform["boundsWidth"], 640.0);
        assert_eq!(transform["cropBottom"], 20);

        // Read back (as OBS reports it), it's the same box.
        let mut reported = transform.clone();
        reported["width"] = json!(640.0);
        reported["height"] = json!(360.0);
        let now = current(&reported, (1920, 1080));
        assert_eq!(
            (now.x, now.y, now.width, now.height),
            (100.0, 50.0, 640.0, 360.0)
        );
        assert_eq!(now.crop, crop);
    }

    #[test]
    fn a_centered_source_without_bounds_reads_from_its_top_left() {
        let now = current(
            &json!({ "boundsType": "OBS_BOUNDS_NONE", "alignment": 0, "positionX": 960.0,
                     "positionY": 540.0, "width": 400.0, "height": 100.0, "rotation": 0.0 }),
            (1920, 1080),
        );
        assert_eq!(
            (now.x, now.y, now.width, now.height),
            (760.0, 490.0, 400.0, 100.0)
        );
    }

    #[test]
    fn out_of_range_placements_are_refused() {
        assert_eq!(
            preset(Fit::Fit, Anchor::Center, 4, 0).check(),
            Err(Invalid::Size)
        );
        assert_eq!(
            preset(Fit::Fit, Anchor::Center, 50, 26).check(),
            Err(Invalid::Margin)
        );
        let exact = |width: f64| Placement::Exact {
            x: 0.0,
            y: 0.0,
            width,
            height: 100.0,
            crop: Crop::default(),
        };
        assert_eq!(exact(0.0).check(), Err(Invalid::Box));
        assert_eq!(exact(f64::NAN).check(), Err(Invalid::Box));
        assert_eq!(exact(100.0).check(), Ok(()));
    }

    fn obs(kind: &str, _: &Value) -> Value {
        match kind {
            "GetVideoSettings" => json!({ "baseWidth": 1920, "baseHeight": 1080,
                "outputWidth": 1280, "outputHeight": 720, "fpsNumerator": 60, "fpsDenominator": 1 }),
            "GetSceneItemList" => {
                json!({ "sceneItems": [input(3, "Chat", "browser_source", true), input(4, "Cam", "dshow_input", false)] })
            }
            "GetSceneItemTransform" => json!({ "sceneItemTransform": {
                "boundsType": "OBS_BOUNDS_NONE", "alignment": 5, "positionX": 0.0, "positionY": 0.0,
                "width": 800.0, "height": 600.0, "rotation": 0.0,
            } }),
            _ => Value::Null,
        }
    }

    #[tokio::test]
    async fn placing_uses_the_canvas_size() {
        let obs = client(obs);
        assert_eq!(
            obs.place("LIVE", false, 3, &preset(Fit::Fit, Anchor::Center, 100, 0))
                .await
                .unwrap()
                .as_deref(),
            Some("Chat")
        );
        let set = &obs.asked(&["SetSceneItemTransform"])[0].1;
        assert_eq!(set["sceneItemTransform"]["boundsWidth"], 1920.0);
        assert_eq!(
            obs.place("LIVE", false, 9, &preset(Fit::Fit, Anchor::Center, 100, 0))
                .await
                .unwrap(),
            None
        );

        let layout = obs.layout("LIVE", false).await.unwrap();
        assert_eq!(layout.canvas, (1920, 1080));
        // Top first: OBS lists Cam last, so it's drawn over Chat.
        let names: Vec<_> = layout
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.enabled))
            .collect();
        assert_eq!(names, [("Cam", false), ("Chat", true)]);
        assert_eq!(layout.items[1].placement.width, 800.0);
    }
}
