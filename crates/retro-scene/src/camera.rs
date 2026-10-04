//! Camera and screen globals used by scripts and the object update loops.
//!
//! v4 keeps one camera (`cameraEnabled`, `cameraTarget`, `cameraStyle`, `cameraXPos`,
//! `cameraYPos`, `cameraAdjustY`, `cameraShakeX/Y`) plus the derived scroll offsets used by
//! `ProcessObjects` (`RSDKv4/Scene.cpp`, RSDKModding/RSDKv4-Decompilation @ a7f5195). The
//! rev03 `screen.camera*`/`camera.*` script variables all alias these globals.

use serde::Serialize;

/// `CAMERASTYLE_*` values used by `screen.cameraStyle`/`camera.style`.
pub const CAMERASTYLE_FOLLOW: i32 = 0;
/// Extended camera style.
pub const CAMERASTYLE_EXTENDED: i32 = 1;
/// Extended camera with a left offset.
pub const CAMERASTYLE_EXTENDED_OFFSET_L: i32 = 2;
/// Extended camera with a right offset.
pub const CAMERASTYLE_EXTENDED_OFFSET_R: i32 = 3;
/// Horizontally locked camera style.
pub const CAMERASTYLE_HLOCKED: i32 = 4;
/// Fixed camera style (`SetPlayerScreenPositionFixed`).
pub const CAMERASTYLE_FIXED: i32 = 5;
/// Static camera style (`SetPlayerScreenPositionStatic`).
pub const CAMERASTYLE_STATIC: i32 = 6;

/// The runtime camera state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Camera {
    /// `cameraEnabled`; `1` means the camera follows its target.
    pub enabled: i32,
    /// `cameraTarget`: entity slot followed by the camera, or `-1`.
    pub target: i32,
    /// `cameraStyle`: one of the `CAMERASTYLE_*` values.
    pub style: i32,
    /// `cameraXPos`, in screen pixels.
    pub xpos: i32,
    /// `cameraYPos`, in screen pixels.
    pub ypos: i32,
    /// `cameraAdjustY` script override.
    pub adjust_y: i32,
    /// `cameraShakeX` (updated by the render loop in upstream).
    pub shake_x: i32,
    /// `cameraShakeY` (updated by the render loop in upstream).
    pub shake_y: i32,
    /// `cameraLockedY`: latches the vertical camera once it settles on its target.
    pub locked_y: i32,
}

impl Camera {
    /// Camera state applied when a scene finishes loading (`STAGEMODE_LOAD` in `ProcessStage`):
    /// enabled, no target, follow style and all offsets zeroed.
    #[must_use]
    pub fn scene_load() -> Self {
        Self {
            enabled: 1,
            target: -1,
            style: CAMERASTYLE_FOLLOW,
            ..Self::default()
        }
    }
}

/// The screen globals the render/object loops derive from the camera and stage bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Screen {
    /// Logical screen width (`SCREEN_XSIZE`).
    pub xsize: i32,
    /// Logical screen height (`SCREEN_YSIZE`).
    pub ysize: i32,
    /// `xScrollOffset` used by `ProcessObjects` and draw positions.
    pub x_scroll: i32,
    /// `yScrollOffset` used by `ProcessObjects` and draw positions.
    pub y_scroll: i32,
}

impl Screen {
    /// Creates a screen with the shipped v4 logical size (424x240) and zero scroll.
    #[must_use]
    pub fn v4() -> Self {
        Self {
            xsize: 424,
            ysize: 240,
            x_scroll: 0,
            y_scroll: 0,
        }
    }

    /// Half the logical width (`SCREEN_CENTERX`).
    #[must_use]
    pub fn center_x(&self) -> i32 {
        self.xsize / 2
    }

    /// Half the logical height (`SCREEN_CENTERY`).
    #[must_use]
    pub fn center_y(&self) -> i32 {
        self.ysize / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_screen_constants_match_upstream() {
        let screen = Screen::v4();
        assert_eq!(screen.xsize, 424);
        assert_eq!(screen.ysize, 240);
        assert_eq!(screen.center_x(), 212);
        assert_eq!(screen.center_y(), 120);
    }

    #[test]
    fn camera_defaults_are_zeroed() {
        let camera = Camera::default();
        assert_eq!(camera.enabled, 0);
        assert_eq!(camera.target, 0);
        assert_eq!(camera.style, CAMERASTYLE_FOLLOW);
        assert_eq!(camera.xpos, 0);
        assert_eq!(camera.adjust_y, 0);
    }

    #[test]
    fn camera_style_constants_match_upstream_enum_order() {
        // `CameraStyles` in `Scene/Legacy/SceneLegacy.hpp:61-70` is a plain sequential enum.
        assert_eq!(CAMERASTYLE_FOLLOW, 0);
        assert_eq!(CAMERASTYLE_EXTENDED, 1);
        assert_eq!(CAMERASTYLE_EXTENDED_OFFSET_L, 2);
        assert_eq!(CAMERASTYLE_EXTENDED_OFFSET_R, 3);
        assert_eq!(CAMERASTYLE_HLOCKED, 4);
        assert_eq!(CAMERASTYLE_FIXED, 5);
        assert_eq!(CAMERASTYLE_STATIC, 6);
    }

    #[test]
    fn scene_load_camera_matches_upstream() {
        let camera = Camera::scene_load();
        assert_eq!(camera.enabled, 1);
        assert_eq!(camera.target, -1);
        assert_eq!(camera.style, CAMERASTYLE_FOLLOW);
        assert_eq!((camera.xpos, camera.ypos), (0, 0));
        assert_eq!((camera.shake_x, camera.shake_y), (0, 0));
    }
}
