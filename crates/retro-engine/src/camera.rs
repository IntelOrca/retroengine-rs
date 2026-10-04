//! Camera styles 0-6 and the locked branch, ported from
//! `RSDKv5/RSDK/Scene/Legacy/v4/SceneLegacyv4.cpp`:
//!
//! * `HandleCameras` (:379-399): target validation, `enabled == 1` style dispatch,
//! * [`set_player_screen_position`] — `SetPlayerScreenPosition` style 0 (:887-1110),
//! * [`set_player_screen_position_cd_style`] — `SetPlayerScreenPositionCDStyle` styles 1-3
//!   (:1112-1472),
//! * [`set_player_h_locked_screen_position`] — `SetPlayerHLockedScreenPosition` style 4
//!   (:1474-1665),
//! * [`set_player_locked_screen_position`] — `SetPlayerLockedScreenPosition` locked branch
//!   (:1667-1754),
//! * [`set_player_screen_position_fixed`] — `SetPlayerScreenPositionFixed` style 5 (:1756-1894),
//! * [`set_player_screen_position_static`] — `SetPlayerScreenPositionStatic` style 6
//!   (:1896-1905).
//!
//! Every function repeats the same four boundary-easing blocks in the same order (Y1, Y2, X1,
//! X2), differing only in two details: `SetPlayerHLockedScreenPosition` also takes the Y1
//! branch when `newYBoundary1 == curYBoundary1`, and `SetPlayerLockedScreenPosition` does not
//! advance the Y2 bound by the target's velocity. [`ease_boundaries`] factors them out.
//!
//! The shipped S1/S2 scripts only assign `CAMERASTYLE_HLOCKED` (spindash/fire-shield abilities)
//! and `CAMERASTYLE_STATIC` (drowning/death under `USE_ORIGINS` + vs mode); nothing assigns the
//! extended styles 1-3 or `CAMERASTYLE_FIXED`, so those are covered by unit tests only.

use retro_scene::ENTITY_COUNT;
use retro_scene::camera::{
    CAMERASTYLE_EXTENDED, CAMERASTYLE_EXTENDED_OFFSET_L, CAMERASTYLE_EXTENDED_OFFSET_R,
    CAMERASTYLE_FIXED, CAMERASTYLE_FOLLOW, CAMERASTYLE_HLOCKED, CAMERASTYLE_STATIC,
};

use crate::state::EngineState;

/// The variants of the repeated boundary-easing block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryEasing {
    /// `SetPlayerScreenPosition`, `...CDStyle` and `...Fixed`: strict `>`/`<` Y1 and a Y2 rise
    /// stepped by the target's Y velocity.
    Follow,
    /// `SetPlayerHLockedScreenPosition`: Y1 also runs when `newYBoundary1 == curYBoundary1`.
    HLocked,
    /// `SetPlayerLockedScreenPosition`: Y2 rises by one without the velocity step.
    Locked,
}

/// `HandleCameras` (`SceneLegacyv4.cpp:379-399`): follows `camera.target` once per frame.
///
/// `target` outside `0..ENTITY_COUNT` returns untouched; `enabled == 1` dispatches by style
/// (unknown styles are ignored); every other `enabled` value runs the locked branch.
pub(crate) fn handle_cameras(state: &mut EngineState) {
    let target = state.camera.target;
    if target < 0 || target as usize >= ENTITY_COUNT {
        return;
    }
    let target = target as usize;
    if state.camera.enabled == 1 {
        match state.camera.style {
            CAMERASTYLE_FOLLOW => set_player_screen_position(state, target),
            CAMERASTYLE_EXTENDED
            | CAMERASTYLE_EXTENDED_OFFSET_L
            | CAMERASTYLE_EXTENDED_OFFSET_R => {
                set_player_screen_position_cd_style(state, target);
            }
            CAMERASTYLE_HLOCKED => set_player_h_locked_screen_position(state, target),
            CAMERASTYLE_FIXED => set_player_screen_position_fixed(state, target),
            CAMERASTYLE_STATIC => set_player_screen_position_static(state, target),
            _ => {}
        }
    } else {
        set_player_locked_screen_position(state, target);
    }
}

/// The shared boundary easing, called in upstream order: Y1, Y2, X1, X2.
fn ease_boundaries(state: &mut EngineState, x_vel: i32, y_vel: i32, mode: BoundaryEasing) {
    let screen_w = state.screen.xsize;
    let screen_h = state.screen.ysize;
    let x_scroll = state.screen.x_scroll;
    let y_scroll = state.screen.y_scroll;

    if mode == BoundaryEasing::HLocked {
        if state.stage.new_y_boundary1 <= state.stage.cur_y_boundary1 {
            if state.stage.cur_y_boundary1 > y_scroll {
                state.stage.cur_y_boundary1 = state.stage.cur_y_boundary1.wrapping_sub(1);
            } else {
                state.stage.cur_y_boundary1 = state.stage.new_y_boundary1;
            }
        } else if state.stage.new_y_boundary1 >= y_scroll {
            state.stage.cur_y_boundary1 = y_scroll;
        } else {
            state.stage.cur_y_boundary1 = state.stage.new_y_boundary1;
        }
    } else {
        if state.stage.new_y_boundary1 > state.stage.cur_y_boundary1 {
            state.stage.cur_y_boundary1 = if state.stage.new_y_boundary1 >= y_scroll {
                y_scroll
            } else {
                state.stage.new_y_boundary1
            };
        }
        if state.stage.new_y_boundary1 < state.stage.cur_y_boundary1 {
            if state.stage.cur_y_boundary1 >= y_scroll {
                state.stage.cur_y_boundary1 = state.stage.cur_y_boundary1.wrapping_sub(1);
            } else {
                state.stage.cur_y_boundary1 = state.stage.new_y_boundary1;
            }
        }
    }

    if state.stage.new_y_boundary2 < state.stage.cur_y_boundary2 {
        if state.stage.cur_y_boundary2 <= y_scroll.wrapping_add(screen_h)
            || state.stage.new_y_boundary2 >= y_scroll.wrapping_add(screen_h)
        {
            state.stage.cur_y_boundary2 = state.stage.cur_y_boundary2.wrapping_sub(1);
        } else {
            state.stage.cur_y_boundary2 = y_scroll.wrapping_add(screen_h);
        }
    }
    if state.stage.new_y_boundary2 > state.stage.cur_y_boundary2 {
        if y_scroll.wrapping_add(screen_h) >= state.stage.cur_y_boundary2 {
            state.stage.cur_y_boundary2 = state.stage.cur_y_boundary2.wrapping_add(1);
            if mode != BoundaryEasing::Locked && y_vel > 0 {
                let buffer = state.stage.cur_y_boundary2.wrapping_add(y_vel >> 16);
                state.stage.cur_y_boundary2 = if state.stage.new_y_boundary2 < buffer {
                    state.stage.new_y_boundary2
                } else {
                    buffer
                };
            }
        } else {
            state.stage.cur_y_boundary2 = state.stage.new_y_boundary2;
        }
    }

    if state.stage.new_x_boundary1 > state.stage.cur_x_boundary1 {
        state.stage.cur_x_boundary1 = if x_scroll <= state.stage.new_x_boundary1 {
            x_scroll
        } else {
            state.stage.new_x_boundary1
        };
    }
    if state.stage.new_x_boundary1 < state.stage.cur_x_boundary1 {
        if x_scroll <= state.stage.cur_x_boundary1 {
            state.stage.cur_x_boundary1 = state.stage.cur_x_boundary1.wrapping_sub(1);
            if x_vel < 0 {
                state.stage.cur_x_boundary1 = state.stage.cur_x_boundary1.wrapping_add(x_vel >> 16);
                if state.stage.cur_x_boundary1 < state.stage.new_x_boundary1 {
                    state.stage.cur_x_boundary1 = state.stage.new_x_boundary1;
                }
            }
        } else {
            state.stage.cur_x_boundary1 = state.stage.new_x_boundary1;
        }
    }

    if state.stage.new_x_boundary2 < state.stage.cur_x_boundary2 {
        state.stage.cur_x_boundary2 =
            if state.stage.new_x_boundary2 > screen_w.wrapping_add(x_scroll) {
                state.stage.new_x_boundary2
            } else {
                screen_w.wrapping_add(x_scroll)
            };
    }
    if state.stage.new_x_boundary2 > state.stage.cur_x_boundary2 {
        if screen_w.wrapping_add(x_scroll) >= state.stage.cur_x_boundary2 {
            state.stage.cur_x_boundary2 = state.stage.cur_x_boundary2.wrapping_add(1);
            if x_vel > 0 {
                state.stage.cur_x_boundary2 = state.stage.cur_x_boundary2.wrapping_add(x_vel >> 16);
                if state.stage.cur_x_boundary2 > state.stage.new_x_boundary2 {
                    state.stage.cur_x_boundary2 = state.stage.new_x_boundary2;
                }
            }
        } else {
            state.stage.cur_x_boundary2 = state.stage.new_x_boundary2;
        }
    }
}

/// The shake decay every style except `STATIC` runs at the end.
fn decay_camera_shake(state: &mut EngineState) {
    if state.camera.shake_x != 0 {
        state.camera.shake_x = if state.camera.shake_x <= 0 {
            !state.camera.shake_x
        } else {
            -state.camera.shake_x
        };
    }
    if state.camera.shake_y != 0 {
        state.camera.shake_y = if state.camera.shake_y <= 0 {
            !state.camera.shake_y
        } else {
            -state.camera.shake_y
        };
    }
}

/// `camera.ypos + dif` unless that falls below `limit`, in which case `limit`.
fn camera_y_plus(camera_y: i32, dif: i32, limit: i32) -> i32 {
    let sum = camera_y.wrapping_add(dif);
    if sum >= limit { sum } else { limit }
}

/// `SetPlayerScreenPosition` (`CAMERASTYLE_FOLLOW`), `SceneLegacyv4.cpp:887-1110`.
fn set_player_screen_position(state: &mut EngineState, target: usize) {
    let Some(entity) = state.entities.get(target).copied() else {
        return;
    };

    let screen_h = state.screen.ysize;
    let half_x = state.screen.center_x();
    let scroll_up = screen_h / 2 - 16;
    let scroll_down = screen_h / 2 + 16;
    let target_x = entity.xpos >> 16;
    let target_y = state.camera.adjust_y.wrapping_add(entity.ypos >> 16);
    let x_vel = entity.xvel;
    let y_vel = entity.yvel;

    ease_boundaries(state, x_vel, y_vel, BoundaryEasing::Follow);

    let mut x_pos_dif = target_x.wrapping_sub(state.camera.xpos);
    if target_x > state.camera.xpos {
        x_pos_dif = x_pos_dif.wrapping_sub(8);
        if x_pos_dif >= 0 {
            if x_pos_dif >= 17 {
                x_pos_dif = 16;
            }
        } else {
            x_pos_dif = 0;
        }
    } else {
        x_pos_dif = x_pos_dif.wrapping_add(8);
        if x_pos_dif > 0 {
            x_pos_dif = 0;
        } else if x_pos_dif <= -17 {
            x_pos_dif = -16;
        }
    }
    let mut centered_x_bound1 = state.camera.xpos.wrapping_add(x_pos_dif);
    state.camera.xpos = centered_x_bound1;
    if centered_x_bound1 < half_x.wrapping_add(state.stage.cur_x_boundary1) {
        state.camera.xpos = half_x.wrapping_add(state.stage.cur_x_boundary1);
        centered_x_bound1 = state.camera.xpos;
    }
    let centered_x_bound2 = state.stage.cur_x_boundary2.wrapping_sub(half_x);
    if centered_x_bound2 < centered_x_bound1 {
        state.camera.xpos = centered_x_bound2;
        centered_x_bound1 = centered_x_bound2;
    }

    let mut y_pos_dif;
    if entity.scroll_tracking != 0 {
        if target_y <= state.camera.ypos {
            y_pos_dif = target_y.wrapping_sub(state.camera.ypos).wrapping_add(32);
            if y_pos_dif <= 0 {
                if y_pos_dif <= -17 {
                    y_pos_dif = -16;
                }
            } else {
                y_pos_dif = 0;
            }
        } else {
            y_pos_dif = target_y.wrapping_sub(state.camera.ypos).wrapping_sub(32);
            if y_pos_dif >= 0 {
                if y_pos_dif >= 17 {
                    y_pos_dif = 16;
                }
            } else {
                y_pos_dif = 0;
            }
        }
        state.camera.locked_y = 0;
    } else if state.camera.locked_y != 0 {
        y_pos_dif = 0;
        state.camera.ypos = target_y;
    } else if target_y <= state.camera.ypos {
        y_pos_dif = target_y.wrapping_sub(state.camera.ypos);
        if target_y.wrapping_sub(state.camera.ypos) <= 0 {
            if y_pos_dif >= -32 && y_vel.unsigned_abs() <= 0x60000 {
                if y_pos_dif < -6 {
                    y_pos_dif = -6;
                }
            } else if y_pos_dif < -16 {
                y_pos_dif = -16;
            }
        } else {
            y_pos_dif = 0;
            state.camera.locked_y = 1;
        }
    } else {
        y_pos_dif = target_y.wrapping_sub(state.camera.ypos);
        if target_y.wrapping_sub(state.camera.ypos) < 0 {
            y_pos_dif = 0;
            state.camera.locked_y = 1;
        } else if y_pos_dif > 32 || y_vel.unsigned_abs() > 0x60000 {
            if y_pos_dif > 16 {
                y_pos_dif = 16;
            } else {
                state.camera.locked_y = 1;
            }
        } else if y_pos_dif <= 6 {
            state.camera.locked_y = 1;
        } else {
            y_pos_dif = 6;
        }
    }

    let mut new_cam_y = state.camera.ypos.wrapping_add(y_pos_dif);
    if new_cam_y
        <= state
            .stage
            .cur_y_boundary1
            .wrapping_add(scroll_up.wrapping_sub(1))
    {
        new_cam_y = state.stage.cur_y_boundary1.wrapping_add(scroll_up);
    }
    state.camera.ypos = new_cam_y;
    if state
        .stage
        .cur_y_boundary2
        .wrapping_sub(scroll_down.wrapping_sub(1))
        <= new_cam_y
    {
        state.camera.ypos = state.stage.cur_y_boundary2.wrapping_sub(scroll_down);
    }

    state.screen.x_scroll = state.camera.shake_x.wrapping_add(centered_x_bound1) - half_x;
    let pos = state
        .camera
        .ypos
        .wrapping_add(entity.look_pos_y)
        .wrapping_sub(scroll_up);
    state.screen.y_scroll = if pos < state.stage.cur_y_boundary1 {
        state.stage.cur_y_boundary1
    } else {
        pos
    };
    let mut y = state.stage.cur_y_boundary2.wrapping_sub(screen_h);
    if state
        .stage
        .cur_y_boundary2
        .wrapping_sub(screen_h.wrapping_sub(1))
        > state.screen.y_scroll
    {
        y = state.screen.y_scroll;
    }
    state.screen.y_scroll = state.camera.shake_y.wrapping_add(y);

    decay_camera_shake(state);
}

/// `SetPlayerScreenPositionCDStyle` (`CAMERASTYLE_EXTENDED`, `..._OFFSET_L`,
/// `..._OFFSET_R`), `SceneLegacyv4.cpp:1112-1472`.
fn set_player_screen_position_cd_style(state: &mut EngineState, target: usize) {
    let Some(entity) = state.entities.get(target).copied() else {
        return;
    };

    let screen_w = state.screen.xsize;
    let screen_h = state.screen.ysize;
    let half_x = state.screen.center_x();
    let scroll_up = screen_h / 2 - 16;
    let scroll_down = screen_h / 2 + 16;
    let target_x = entity.xpos >> 16;
    let target_y = state.camera.adjust_y.wrapping_add(entity.ypos >> 16);
    let x_vel = entity.xvel;
    let y_vel = entity.yvel;

    ease_boundaries(state, x_vel, y_vel, BoundaryEasing::Follow);

    let mut look_pos_x = entity.look_pos_x;
    if entity.gravity == 0 {
        if entity.direction != 0 {
            if state.camera.style == CAMERASTYLE_EXTENDED_OFFSET_R || entity.speed < -0x5F5C2 {
                state.camera_shift = 2;
                if look_pos_x <= 63 {
                    look_pos_x += 2;
                }
            } else {
                state.camera_shift = 0;
                if look_pos_x < 0 {
                    look_pos_x += 2;
                }
                if look_pos_x > 0 {
                    look_pos_x -= 2;
                }
            }
        } else if state.camera.style == CAMERASTYLE_EXTENDED_OFFSET_L || entity.speed > 0x5F5C2 {
            state.camera_shift = 1;
            if look_pos_x >= -63 {
                look_pos_x -= 2;
            }
        } else {
            state.camera_shift = 0;
            if look_pos_x < 0 {
                look_pos_x += 2;
            }
            if look_pos_x > 0 {
                look_pos_x -= 2;
            }
        }
    } else if state.camera_shift == 1 {
        if look_pos_x >= -63 {
            look_pos_x -= 2;
        }
    } else if state.camera_shift < 1 {
        if look_pos_x < 0 {
            look_pos_x += 2;
        }
        if look_pos_x > 0 {
            look_pos_x -= 2;
        }
    } else if state.camera_shift == 2 && look_pos_x <= 63 {
        look_pos_x += 2;
    }
    if let Some(target_entity) = state.entities.get_mut(target) {
        target_entity.look_pos_x = look_pos_x;
    }
    state.camera.xpos = target_x.wrapping_sub(look_pos_x);

    if entity.scroll_tracking == 0 {
        if state.camera.locked_y != 0 {
            state.camera.ypos = target_y;
            if state.camera.ypos < state.stage.cur_y_boundary1.wrapping_add(scroll_up) {
                state.camera.ypos = state.stage.cur_y_boundary1.wrapping_add(scroll_up);
            }
        } else if target_y > state.camera.ypos {
            let mut dif = target_y.wrapping_sub(state.camera.ypos);
            if target_y.wrapping_sub(state.camera.ypos) < 0 {
                state.camera.locked_y = 1;
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    0,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            } else {
                if dif > 32 || y_vel.unsigned_abs() > 0x60000 {
                    if dif > 16 {
                        dif = 16;
                    } else {
                        state.camera.locked_y = 1;
                    }
                } else if dif > 6 {
                    dif = 6;
                } else {
                    state.camera.locked_y = 1;
                }
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    dif,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            }
        } else {
            let mut dif = target_y.wrapping_sub(state.camera.ypos);
            if target_y.wrapping_sub(state.camera.ypos) <= 0 {
                if dif < -32 || y_vel.unsigned_abs() > 0x60000 {
                    if dif < -16 {
                        dif = -16;
                    } else {
                        state.camera.locked_y = 1;
                    }
                } else if dif < -6 {
                    dif = -6;
                }
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    dif,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            } else {
                dif = 0;
                // Upstream branches on `abs(target->yvel) > 0x60000` here but sets
                // `cameraLockedY = true` in both arms, so the branch has no effect.
                state.camera.locked_y = 1;
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    dif,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            }
        }
    } else {
        let dif = target_y.wrapping_sub(state.camera.ypos);
        let mut dif_y;
        if target_y > state.camera.ypos {
            dif_y = dif.wrapping_sub(32);
            if dif_y >= 0 {
                if dif_y >= 17 {
                    dif_y = 16;
                }
                state.camera.locked_y = 0;
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    dif_y,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            } else {
                state.camera.locked_y = 0;
                if state.camera.ypos < state.stage.cur_y_boundary1.wrapping_add(scroll_up) {
                    state.camera.ypos = state.stage.cur_y_boundary1.wrapping_add(scroll_up);
                }
            }
        } else {
            dif_y = dif.wrapping_add(32);
            if dif_y > 0 {
                // Upstream zeroes `difY` here but never reads it (only the floor clamp runs).
                state.camera.locked_y = 0;
                if state.camera.ypos < state.stage.cur_y_boundary1.wrapping_add(scroll_up) {
                    state.camera.ypos = state.stage.cur_y_boundary1.wrapping_add(scroll_up);
                }
            } else if dif_y <= -17 {
                dif_y = -16;
                state.camera.locked_y = 0;
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    dif_y,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            } else {
                state.camera.locked_y = 0;
                state.camera.ypos = camera_y_plus(
                    state.camera.ypos,
                    dif_y,
                    state.stage.cur_y_boundary1.wrapping_add(scroll_up),
                );
            }
        }
    }

    if state.camera.ypos
        >= state
            .stage
            .cur_y_boundary2
            .wrapping_sub(scroll_down)
            .wrapping_sub(1)
    {
        state.camera.ypos = state.stage.cur_y_boundary2.wrapping_sub(scroll_down);
    }

    state.screen.x_scroll = state.camera.xpos.wrapping_sub(half_x);
    state.screen.y_scroll = entity
        .look_pos_y
        .wrapping_add(state.camera.ypos)
        .wrapping_sub(scroll_up);

    let mut x = state.stage.cur_x_boundary1;
    if x <= state.screen.x_scroll {
        x = state.screen.x_scroll;
    } else {
        state.screen.x_scroll = x;
    }
    if x > state.stage.cur_x_boundary2.wrapping_sub(screen_w) {
        x = state.stage.cur_x_boundary2.wrapping_sub(screen_w);
        state.screen.x_scroll = state.stage.cur_x_boundary2.wrapping_sub(screen_w);
    }
    let mut y = state.stage.cur_y_boundary1;
    if state.screen.y_scroll >= y {
        y = state.screen.y_scroll;
    } else {
        state.screen.y_scroll = y;
    }
    if state
        .stage
        .cur_y_boundary2
        .wrapping_sub(screen_h)
        .wrapping_sub(1)
        <= y
    {
        y = state.stage.cur_y_boundary2.wrapping_sub(screen_h);
    }
    state.screen.x_scroll = state.camera.shake_x.wrapping_add(x);
    state.screen.y_scroll = state.camera.shake_y.wrapping_add(y);

    decay_camera_shake(state);
}

/// `SetPlayerHLockedScreenPosition` (`CAMERASTYLE_HLOCKED`), `SceneLegacyv4.cpp:1474-1665`.
fn set_player_h_locked_screen_position(state: &mut EngineState, target: usize) {
    let Some(entity) = state.entities.get(target).copied() else {
        return;
    };

    let screen_h = state.screen.ysize;
    let scroll_up = screen_h / 2 - 16;
    let scroll_down = screen_h / 2 + 16;
    let target_y = state.camera.adjust_y.wrapping_add(entity.ypos >> 16);
    let y_vel = entity.yvel;

    ease_boundaries(state, entity.xvel, y_vel, BoundaryEasing::HLocked);

    let mut cam_scroll;
    if entity.scroll_tracking != 0 {
        if target_y <= state.camera.ypos {
            cam_scroll = target_y.wrapping_sub(state.camera.ypos).wrapping_add(32);
            if cam_scroll <= 0 {
                if cam_scroll <= -17 {
                    cam_scroll = -16;
                }
            } else {
                cam_scroll = 0;
            }
        } else {
            cam_scroll = target_y.wrapping_sub(state.camera.ypos).wrapping_sub(32);
            if cam_scroll >= 0 {
                if cam_scroll >= 17 {
                    cam_scroll = 16;
                }
            } else {
                cam_scroll = 0;
            }
        }
        state.camera.locked_y = 0;
    } else if state.camera.locked_y != 0 {
        cam_scroll = 0;
        state.camera.ypos = target_y;
    } else if target_y > state.camera.ypos {
        cam_scroll = target_y.wrapping_sub(state.camera.ypos);
        if cam_scroll >= 0 {
            if cam_scroll > 32 || y_vel.unsigned_abs() > 0x60000 {
                if cam_scroll > 16 {
                    cam_scroll = 16;
                } else {
                    state.camera.locked_y = 1;
                }
            } else if cam_scroll > 6 {
                cam_scroll = 6;
            } else {
                state.camera.locked_y = 1;
            }
        } else {
            cam_scroll = 0;
            state.camera.locked_y = 1;
        }
    } else {
        cam_scroll = target_y.wrapping_sub(state.camera.ypos);
        if cam_scroll > 0 {
            cam_scroll = 0;
            state.camera.locked_y = 1;
        } else if cam_scroll < -32 || y_vel.unsigned_abs() > 0x60000 {
            if cam_scroll < -16 {
                cam_scroll = -16;
            } else {
                state.camera.locked_y = 1;
            }
        } else if cam_scroll >= -6 {
            state.camera.locked_y = 1;
        } else {
            cam_scroll = -6;
        }
    }

    let mut new_cam_y = state.camera.ypos.wrapping_add(cam_scroll);
    if new_cam_y
        <= state
            .stage
            .cur_y_boundary1
            .wrapping_add(scroll_up.wrapping_sub(1))
    {
        new_cam_y = state.stage.cur_y_boundary1.wrapping_add(scroll_up);
    }
    state.camera.ypos = new_cam_y;
    if state
        .stage
        .cur_y_boundary2
        .wrapping_sub(scroll_down.wrapping_sub(1))
        <= new_cam_y
    {
        new_cam_y = state.stage.cur_y_boundary2.wrapping_sub(scroll_down);
        state.camera.ypos = state.stage.cur_y_boundary2.wrapping_sub(scroll_down);
    }

    state.screen.x_scroll = state
        .camera
        .shake_x
        .wrapping_add(state.camera.xpos)
        .wrapping_sub(state.screen.center_x());

    let pos = new_cam_y
        .wrapping_add(entity.look_pos_y)
        .wrapping_sub(scroll_up);
    state.screen.y_scroll = if pos < state.stage.cur_y_boundary1 {
        state.stage.cur_y_boundary1
    } else {
        new_cam_y
            .wrapping_add(entity.look_pos_y)
            .wrapping_sub(scroll_up)
    };
    let y1 = state
        .stage
        .cur_y_boundary2
        .wrapping_sub(screen_h.wrapping_sub(1));
    let mut y2 = state.stage.cur_y_boundary2.wrapping_sub(screen_h);
    if y1 > state.screen.y_scroll {
        y2 = state.screen.y_scroll;
    }
    state.screen.y_scroll = state.camera.shake_y.wrapping_add(y2);

    decay_camera_shake(state);
}

/// `SetPlayerLockedScreenPosition` (`cameraEnabled != 1`), `SceneLegacyv4.cpp:1667-1754`.
fn set_player_locked_screen_position(state: &mut EngineState, target: usize) {
    let Some(entity) = state.entities.get(target).copied() else {
        return;
    };

    ease_boundaries(state, entity.xvel, entity.yvel, BoundaryEasing::Locked);
    decay_camera_shake(state);
}

/// `SetPlayerScreenPositionFixed` (`CAMERASTYLE_FIXED`), `SceneLegacyv4.cpp:1756-1894`.
fn set_player_screen_position_fixed(state: &mut EngineState, target: usize) {
    let Some(entity) = state.entities.get(target).copied() else {
        return;
    };

    let screen_h = state.screen.ysize;
    let half_x = state.screen.center_x();
    let half_y = state.screen.center_y();
    let mut target_x = entity.xpos >> 16;
    let mut target_y = state.camera.adjust_y.wrapping_add(entity.ypos >> 16);

    ease_boundaries(state, entity.xvel, entity.yvel, BoundaryEasing::Follow);

    state.camera.xpos = target_x;
    if target_x < half_x.wrapping_add(state.stage.cur_x_boundary1) {
        target_x = half_x.wrapping_add(state.stage.cur_x_boundary1);
        state.camera.xpos = half_x.wrapping_add(state.stage.cur_x_boundary1);
    }
    let bound_x2 = state.stage.cur_x_boundary2.wrapping_sub(half_x);
    if bound_x2 < target_x {
        target_x = bound_x2;
        state.camera.xpos = bound_x2;
    }

    if target_y <= state.stage.cur_y_boundary1.wrapping_add(half_y - 1) {
        target_y = state.stage.cur_y_boundary1.wrapping_add(half_y);
        state.camera.ypos = state.stage.cur_y_boundary1.wrapping_add(half_y);
    } else {
        state.camera.ypos = target_y;
    }
    if state
        .stage
        .cur_y_boundary2
        .wrapping_sub(half_y.wrapping_sub(1))
        <= target_y
    {
        target_y = state.stage.cur_y_boundary2.wrapping_sub(half_y);
        state.camera.ypos = state.stage.cur_y_boundary2.wrapping_sub(half_y);
    }

    state.screen.x_scroll = state.camera.shake_x.wrapping_add(target_x) - half_x;
    let cam_y = target_y
        .wrapping_add(entity.look_pos_y)
        .wrapping_sub(half_y);
    state.screen.y_scroll = if state.stage.cur_y_boundary1 > cam_y {
        state.stage.cur_y_boundary1
    } else {
        target_y
            .wrapping_add(entity.look_pos_y)
            .wrapping_sub(half_y)
    };

    let mut new_cam_y = state.stage.cur_y_boundary2.wrapping_sub(screen_h);
    if state
        .stage
        .cur_y_boundary2
        .wrapping_sub(screen_h.wrapping_sub(1))
        > state.screen.y_scroll
    {
        new_cam_y = state.screen.y_scroll;
    }
    state.screen.y_scroll = state.camera.shake_y.wrapping_add(new_cam_y);

    decay_camera_shake(state);
}

/// `SetPlayerScreenPositionStatic` (`CAMERASTYLE_STATIC`), `SceneLegacyv4.cpp:1896-1905`.
fn set_player_screen_position_static(state: &mut EngineState, target: usize) {
    let Some(entity) = state.entities.get(target).copied() else {
        return;
    };

    let screen_h = state.screen.ysize;
    let scroll_up = screen_h / 2 - 16;

    state.screen.x_scroll = state.camera.xpos.wrapping_sub(state.screen.center_x());
    state.screen.y_scroll = state
        .camera
        .ypos
        .wrapping_add(entity.look_pos_y)
        .wrapping_sub(scroll_up);

    if state.screen.y_scroll < state.stage.cur_y_boundary1 {
        state.screen.y_scroll = state.stage.cur_y_boundary1;
    }
    if state.screen.y_scroll > state.stage.cur_y_boundary2.wrapping_sub(screen_h) {
        state.screen.y_scroll = state.stage.cur_y_boundary2.wrapping_sub(screen_h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeProfile;
    use crate::profile::EngineSettings;
    use crate::rng::GlibcRand;
    use retro_format_v4::{GameConfig, Scene, StageConfig};
    use retro_io::MemorySource;
    use retro_scene::ObjectRegistry;
    use retro_script::{PlatformMode, V4Revision};
    use std::sync::Arc;

    fn state() -> EngineState {
        EngineState::new(
            Arc::new(MemorySource::new()),
            EngineSettings {
                profile: RuntimeProfile::V4Legacy,
                platform: PlatformMode::Standalone,
                revision: V4Revision::Rev03,
                force_scripts: false,
                dim_limit_frames: 18000,
            },
            GameConfig {
                title: "Test".to_owned(),
                subtitle: String::new(),
                palette: vec![[0, 0, 0]; retro_format_v4::gameconfig::PALETTE_COUNT],
                objects: Vec::new(),
                global_variables: Vec::new(),
                sound_effects: Vec::new(),
                players: Vec::new(),
                categories: retro_format_v4::gameconfig::CATEGORY_NAMES
                    .iter()
                    .map(|name| retro_format_v4::SceneCategory {
                        name: (*name).to_owned(),
                        scenes: Vec::new(),
                    })
                    .collect(),
            },
            "Zone01".to_owned(),
            "1".to_owned(),
            Scene {
                title: "Test".to_owned(),
                active_layers: [9, 9, 9, 9],
                mid_point: 3,
                width: 1,
                height: 1,
                layout: vec![0],
                entities: Vec::new(),
            },
            StageConfig {
                load_global_objects: false,
                palette: vec![[0, 0, 0]; retro_format_v4::stageconfig::STAGE_PALETTE_COUNT],
                sound_effects: Vec::new(),
                objects: Vec::new(),
            },
            None,
            None,
            ObjectRegistry::new(),
            GlibcRand::new(1),
        )
    }

    /// Places the target entity at pixel `(x, y)` in slot 0 and points the camera at it.
    fn target(x: i32, y: i32) -> EngineState {
        let mut state = state();
        state
            .entities
            .reset_object_entity(0, 1, 0, x << 16, y << 16);
        state.camera.target = 0;
        state.camera.enabled = 1;
        state.camera.style = CAMERASTYLE_FOLLOW;
        state
    }

    /// Wide boundaries so that the boundary clamps stay out of the way.
    fn wide_bounds(state: &mut EngineState) {
        state.stage.new_x_boundary1 = 0;
        state.stage.new_x_boundary2 = 10_000;
        state.stage.new_y_boundary1 = 0;
        state.stage.new_y_boundary2 = 10_000;
        state.stage.cur_x_boundary1 = 0;
        state.stage.cur_x_boundary2 = 10_000;
        state.stage.cur_y_boundary1 = 0;
        state.stage.cur_y_boundary2 = 10_000;
    }

    fn set_target(state: &mut EngineState, f: impl FnOnce(&mut retro_scene::Entity)) {
        if let Some(entity) = state.entities.get_mut(0) {
            f(entity);
        }
    }

    fn target_entity(state: &EngineState) -> retro_scene::Entity {
        state.entities.get(0).copied().unwrap_or_default()
    }

    #[test]
    fn style_0_dead_zone_follow_matches_upstream() {
        // `SetPlayerScreenPosition`: `xPosDif` carries an 8px dead zone and a 16px step.
        let mut state = target(300, 500);
        wide_bounds(&mut state);
        state.camera.xpos = 212;
        state.camera.ypos = 500;
        handle_cameras(&mut state);
        // 300 - 212 = 88, minus the 8px dead zone clamps to +16.
        assert_eq!((state.camera.xpos, state.camera.ypos), (228, 500));
        // xScroll = 228 - 212 = 16; yScroll = 500 - 104 = 396 (unclamped).
        assert_eq!((state.screen.x_scroll, state.screen.y_scroll), (16, 396));
    }

    #[test]
    fn cd_style_offset_r_shifts_look_pos_right() {
        // `SceneLegacyv4.cpp:1199`: EXTENDED_OFFSET_R (or speed < -0x5F5C2) with facing right.
        let mut state = target(212, 500);
        wide_bounds(&mut state);
        state.camera.style = CAMERASTYLE_EXTENDED_OFFSET_R;
        state.camera.ypos = 500;
        set_target(&mut state, |entity| {
            entity.gravity = 0;
            entity.direction = 1;
            entity.look_pos_x = 0;
        });
        handle_cameras(&mut state);
        assert_eq!(state.camera_shift, 2);
        assert_eq!(target_entity(&state).look_pos_x, 2);
        // cameraXPos = targetX - lookPosX = 212 - 2; xScroll clamps up to curXBoundary1 (0).
        assert_eq!(state.camera.xpos, 210);
        assert_eq!(state.screen.x_scroll, 0);
        assert_eq!(state.screen.y_scroll, 396);
    }

    #[test]
    fn cd_style_offset_r_speed_branch_stops_at_63() {
        // Style 1 keeps the offset-R speed branch but `lookPosX <= 63` refuses to grow.
        let mut state = target(212, 500);
        wide_bounds(&mut state);
        state.camera.style = 1; // CAMERASTYLE_EXTENDED
        state.camera.ypos = 500;
        set_target(&mut state, |entity| {
            entity.gravity = 0;
            entity.direction = 1;
            entity.speed = -0x60000; // < -0x5F5C2
            entity.look_pos_x = 64;
        });
        handle_cameras(&mut state);
        assert_eq!(state.camera_shift, 2);
        assert_eq!(target_entity(&state).look_pos_x, 64);
        assert_eq!(state.camera.xpos, 212 - 64);
    }

    #[test]
    fn cd_style_offset_l_shifts_look_pos_left() {
        // `SceneLegacyv4.cpp:1217`: EXTENDED_OFFSET_L (or speed > 0x5F5C2) with facing left.
        let mut state = target(212, 500);
        wide_bounds(&mut state);
        state.camera.style = CAMERASTYLE_EXTENDED_OFFSET_L;
        state.camera.ypos = 500;
        set_target(&mut state, |entity| {
            entity.gravity = 0;
            entity.direction = 0;
            entity.look_pos_x = -5;
        });
        handle_cameras(&mut state);
        assert_eq!(state.camera_shift, 1);
        assert_eq!(target_entity(&state).look_pos_x, -7);
        // cameraXPos = 212 + 7; xScroll = 7 (clamped up from -7 to curXBoundary1 + 0... it is
        // positive, and below `curXBoundary2 - SCREEN_XSIZE`).
        assert_eq!(state.camera.xpos, 219);
        assert_eq!(state.screen.x_scroll, 7);
    }

    #[test]
    fn cd_style_offset_l_speed_branch() {
        let mut state = target(212, 500);
        wide_bounds(&mut state);
        state.camera.style = 1;
        state.camera.ypos = 500;
        set_target(&mut state, |entity| {
            entity.gravity = 0;
            entity.direction = 0;
            entity.speed = 0x60000; // > 0x5F5C2
            entity.look_pos_x = 0;
        });
        handle_cameras(&mut state);
        assert_eq!(state.camera_shift, 1);
        assert_eq!(target_entity(&state).look_pos_x, -2);
        assert_eq!(state.camera.xpos, 214);
    }

    #[test]
    fn cd_style_plain_extended_eases_look_pos_toward_zero() {
        let mut state = target(212, 500);
        wide_bounds(&mut state);
        state.camera.style = 1;
        state.camera.ypos = 500;
        set_target(&mut state, |entity| {
            entity.gravity = 0;
            entity.direction = 1;
            entity.look_pos_x = 5;
        });
        handle_cameras(&mut state);
        assert_eq!(state.camera_shift, 0);
        assert_eq!(target_entity(&state).look_pos_x, 3);
        assert_eq!(state.camera.xpos, 209);

        // A second frame keeps easing with no gravity.
        set_target(&mut state, |entity| {
            entity.look_pos_x = 1;
        });
        handle_cameras(&mut state);
        assert_eq!(target_entity(&state).look_pos_x, -1);
        assert_eq!(state.camera.xpos, 213);
    }

    #[test]
    fn cd_style_gravity_continues_the_latched_shift() {
        // With gravity, `cameraShift` alone drives lookPosX (SceneLegacyv4.cpp:1236-1253).
        let mut state = target(212, 500);
        wide_bounds(&mut state);
        state.camera.style = CAMERASTYLE_EXTENDED_OFFSET_L;
        state.camera.ypos = 500;
        set_target(&mut state, |entity| {
            entity.gravity = 0;
            entity.direction = 0;
            entity.look_pos_x = 0;
        });
        handle_cameras(&mut state);
        assert_eq!(state.camera_shift, 1);
        assert_eq!(target_entity(&state).look_pos_x, -2);

        set_target(&mut state, |entity| {
            entity.gravity = 1;
            entity.direction = 0;
        });
        handle_cameras(&mut state);
        assert_eq!(target_entity(&state).look_pos_x, -4);
        assert_eq!(state.camera.xpos, 216);
    }

    #[test]
    fn hlocked_follows_y_only() {
        // `SetPlayerHLockedScreenPosition`: X is untouched; the vertical window is 32px with a
        // 6px/16px step ladder.
        let mut state = target(212, 520);
        wide_bounds(&mut state);
        state.camera.style = CAMERASTYLE_HLOCKED;
        state.camera.xpos = 100;
        state.camera.ypos = 500;
        handle_cameras(&mut state);
        assert_eq!((state.camera.xpos, state.camera.ypos), (100, 506));
        assert_eq!(state.camera.locked_y, 0);
        // xScroll = shake + cameraXPos - SCREEN_CENTERX; yScroll = newCamY - SCROLL_UP.
        assert_eq!(state.screen.x_scroll, 100 - 212);
        assert_eq!(state.screen.y_scroll, 506 - 104);

        // The `cameraLockedY` latch snaps Y to the target before clamping.
        state.camera.locked_y = 1;
        state.camera.ypos = 500;
        state.screen.x_scroll = 0;
        state.screen.y_scroll = 0;
        set_target(&mut state, |entity| entity.ypos = 520 << 16);
        handle_cameras(&mut state);
        assert_eq!(state.camera.ypos, 520);
        assert_eq!(state.screen.y_scroll, 520 - 104);
    }

    #[test]
    fn hlocked_target_below_camera_steps_back_six() {
        let mut state = target(212, 480);
        wide_bounds(&mut state);
        state.camera.style = CAMERASTYLE_HLOCKED;
        state.camera.xpos = 100;
        state.camera.ypos = 500;
        handle_cameras(&mut state);
        // camScroll = -20, in (-32, -6): -20 < -6 so the else branch steps by -6.
        assert_eq!(state.camera.ypos, 494);
    }

    #[test]
    fn static_clamps_y_scroll_without_touching_bounds() {
        // `SetPlayerScreenPositionStatic` only derives the scroll from the frozen camera.
        let mut state = target(212, 500);
        state.camera.style = CAMERASTYLE_STATIC;
        state.camera.xpos = 500;
        state.camera.ypos = 200;
        state.stage.new_y_boundary1 = 2000; // must NOT be eased in
        state.stage.cur_y_boundary1 = 100;
        state.stage.cur_y_boundary2 = 1000;
        state.stage.cur_x_boundary1 = 0;
        state.stage.cur_x_boundary2 = 10_000;
        handle_cameras(&mut state);
        assert_eq!((state.camera.xpos, state.camera.ypos), (500, 200));
        assert_eq!(state.stage.cur_y_boundary1, 100);
        assert_eq!(state.screen.x_scroll, 500 - 212);
        // 200 - 104 = 96 clamps up to curYBoundary1.
        assert_eq!(state.screen.y_scroll, 100);

        // 900 + lookPosY(0) - 104 = 796 clamps down to curYBoundary2 - SCREEN_YSIZE.
        state.camera.ypos = 900;
        handle_cameras(&mut state);
        assert_eq!(state.screen.y_scroll, 760);
    }

    #[test]
    fn fixed_centers_on_the_target_and_clamps_both_axes() {
        let mut state = target(100, 300);
        state.camera.style = CAMERASTYLE_FIXED;
        state.stage.new_x_boundary1 = 0;
        state.stage.new_x_boundary2 = 1000;
        state.stage.new_y_boundary1 = 0;
        state.stage.new_y_boundary2 = 1000;
        state.stage.cur_x_boundary1 = 0;
        state.stage.cur_x_boundary2 = 1000;
        state.stage.cur_y_boundary1 = 0;
        state.stage.cur_y_boundary2 = 1000;
        handle_cameras(&mut state);
        // X clamps up to SCREEN_CENTERX + curXBoundary1 = 212; yScroll = 300 - 120 = 180.
        assert_eq!((state.camera.xpos, state.camera.ypos), (212, 300));
        assert_eq!((state.screen.x_scroll, state.screen.y_scroll), (0, 180));

        // X clamps down to curXBoundary2 - SCREEN_CENTERX.
        set_target(&mut state, |entity| entity.xpos = 900 << 16);
        handle_cameras(&mut state);
        assert_eq!(state.camera.xpos, 788);
        assert_eq!(state.screen.x_scroll, 576);

        // Target above the top: camera and target clamp to curYBoundary1 + 120.
        set_target(&mut state, |entity| entity.ypos = 50 << 16);
        handle_cameras(&mut state);
        assert_eq!(target_entity(&state).ypos, 50 << 16);
        assert_eq!((state.camera.ypos, state.screen.y_scroll), (120, 0));

        // Target below the bottom: both clamp to curYBoundary2 - 120.
        set_target(&mut state, |entity| entity.ypos = 950 << 16);
        handle_cameras(&mut state);
        assert_eq!((state.camera.ypos, state.screen.y_scroll), (880, 760));
    }

    #[test]
    fn locked_branch_eases_bounds_and_decays_shake() {
        // `enabled != 1` runs `SetPlayerLockedScreenPosition`: bounds move, scroll does not.
        let mut state = target(212, 500);
        state.camera.enabled = 0;
        state.camera.xpos = 321;
        state.camera.ypos = 654;
        state.camera.shake_x = 3;
        state.camera.shake_y = -5;
        state.screen.x_scroll = 44;
        state.screen.y_scroll = -7;
        state.stage.new_x_boundary1 = 0;
        state.stage.new_x_boundary2 = 5000;
        state.stage.new_y_boundary1 = 0;
        state.stage.new_y_boundary2 = 5000;
        state.stage.cur_x_boundary1 = 100;
        state.stage.cur_x_boundary2 = 100;
        state.stage.cur_y_boundary1 = 0;
        state.stage.cur_y_boundary2 = 200;
        set_target(&mut state, |entity| {
            entity.xvel = -0x10000;
            entity.yvel = 0x20000;
        });
        handle_cameras(&mut state);
        // X1 (newXBoundary1 = 0, xScroll = 44 <= cur = 100) falls by one, then by
        // xvel>>16 = -1; 98 is not below newXBoundary1, so it stays there.
        assert_eq!(state.stage.cur_x_boundary1, 98);
        // X2 rises by one; the locked branch does NOT add xvel>>16.
        assert_eq!(state.stage.cur_x_boundary2, 101);
        // Y2 rises by one without the yvel step (CD/Fixed would add +2).
        assert_eq!(state.stage.cur_y_boundary2, 201);
        assert_eq!((state.camera.xpos, state.camera.ypos), (321, 654));
        assert_eq!((state.screen.x_scroll, state.screen.y_scroll), (44, -7));
        // Shake decays: 3 -> ~3 = -3, -5 -> ~-5 = 4.
        assert_eq!((state.camera.shake_x, state.camera.shake_y), (-3, 4));
    }

    #[test]
    fn locked_branch_runs_for_every_enabled_value_but_one() {
        for enabled in [0, 2, 255] {
            let mut state = target(212, 500);
            state.camera.enabled = enabled;
            state.camera.shake_x = 1;
            handle_cameras(&mut state);
            assert_eq!(state.camera.shake_x, -1, "enabled = {enabled}");
        }
    }

    #[test]
    fn invalid_targets_leave_everything_alone() {
        for target_value in [-1, ENTITY_COUNT as i32, i32::MIN, i32::MAX] {
            let mut state = target(212, 500);
            state.camera.target = target_value;
            state.camera.shake_x = 3;
            state.camera.shake_y = -5;
            state.stage.new_x_boundary1 = 2000;
            state.screen.x_scroll = 44;
            handle_cameras(&mut state);
            assert_eq!((state.camera.xpos, state.camera.ypos), (0, 0));
            assert_eq!(
                (state.camera.shake_x, state.camera.shake_y),
                (3, -5),
                "target = {target_value}"
            );
            assert_eq!(state.stage.cur_x_boundary1, 0, "target = {target_value}");
            assert_eq!(state.screen.x_scroll, 44, "target = {target_value}");
        }
    }

    #[test]
    fn unknown_style_is_ignored_when_enabled() {
        let mut state = target(212, 500);
        state.camera.style = 7;
        state.camera.shake_x = 3;
        handle_cameras(&mut state);
        assert_eq!(state.camera.shake_x, 3);
        assert_eq!((state.camera.xpos, state.camera.ypos), (0, 0));
    }

    #[test]
    fn dispatch_and_style_0_target_entity_helpers_are_consistent() {
        // Sanity: `target()` leaves a usable entity in slot 0.
        let state = target(212, 500);
        assert_eq!(target_entity(&state).xpos, 212 << 16);
        assert_eq!(state.camera.target, 0);
    }
}
