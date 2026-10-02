//! Opcode tables for RSDKv4 script bytecode.
//!
//! Generated from `RSDKv4/Script.cpp` (`enum ScrFunc` + `FunctionInfo functions[]`) in
//! RSDKModding/RSDKv4-Decompilation @ a7f5195e21fdad7b75e4587e249013feeea9e6f3 (main, 2026-09-06).
//! Do not edit by hand.

use serde::Serialize;

use crate::version::{ScriptVersion, V4Revision};

/// Canonical script operation, independent of revision-specific encoded indices.
///
/// Variants are ordered by first appearance across revisions (rev00 enum order, then later
/// additions). The encoded opcode index for a given revision comes from [`opcode_table`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Op {
    /// `FUNC_END` ("End")
    End,
    /// `FUNC_EQUAL` ("Equal")
    Equal,
    /// `FUNC_ADD` ("Add")
    Add,
    /// `FUNC_SUB` ("Sub")
    Sub,
    /// `FUNC_INC` ("Inc")
    Inc,
    /// `FUNC_DEC` ("Dec")
    Dec,
    /// `FUNC_MUL` ("Mul")
    Mul,
    /// `FUNC_DIV` ("Div")
    Div,
    /// `FUNC_SHR` ("ShR")
    ShR,
    /// `FUNC_SHL` ("ShL")
    ShL,
    /// `FUNC_AND` ("And")
    And,
    /// `FUNC_OR` ("Or")
    Or,
    /// `FUNC_XOR` ("Xor")
    Xor,
    /// `FUNC_MOD` ("Mod")
    Mod,
    /// `FUNC_FLIPSIGN` ("FlipSign")
    FlipSign,
    /// `FUNC_CHECKEQUAL` ("CheckEqual")
    CheckEqual,
    /// `FUNC_CHECKGREATER` ("CheckGreater")
    CheckGreater,
    /// `FUNC_CHECKLOWER` ("CheckLower")
    CheckLower,
    /// `FUNC_CHECKNOTEQUAL` ("CheckNotEqual")
    CheckNotEqual,
    /// `FUNC_IFEQUAL` ("IfEqual")
    IfEqual,
    /// `FUNC_IFGREATER` ("IfGreater")
    IfGreater,
    /// `FUNC_IFGREATEROREQUAL` ("IfGreaterOrEqual")
    IfGreaterOrEqual,
    /// `FUNC_IFLOWER` ("IfLower")
    IfLower,
    /// `FUNC_IFLOWEROREQUAL` ("IfLowerOrEqual")
    IfLowerOrEqual,
    /// `FUNC_IFNOTEQUAL` ("IfNotEqual")
    IfNotEqual,
    /// `FUNC_ELSE` ("else")
    Else,
    /// `FUNC_ENDIF` ("endif")
    EndIf,
    /// `FUNC_WEQUAL` ("WEqual")
    WEqual,
    /// `FUNC_WGREATER` ("WGreater")
    WGreater,
    /// `FUNC_WGREATEROREQUAL` ("WGreaterOrEqual")
    WGreaterOrEqual,
    /// `FUNC_WLOWER` ("WLower")
    WLower,
    /// `FUNC_WLOWEROREQUAL` ("WLowerOrEqual")
    WLowerOrEqual,
    /// `FUNC_WNOTEQUAL` ("WNotEqual")
    WNotEqual,
    /// `FUNC_LOOP` ("loop")
    Loop,
    /// `FUNC_FOREACHACTIVE` ("ForEachActive")
    ForEachActive,
    /// `FUNC_FOREACHALL` ("ForEachAll")
    ForEachAll,
    /// `FUNC_NEXT` ("next")
    Next,
    /// `FUNC_SWITCH` ("switch")
    Switch,
    /// `FUNC_BREAK` ("break")
    Break,
    /// `FUNC_ENDSWITCH` ("endswitch")
    EndSwitch,
    /// `FUNC_RAND` ("Rand")
    Rand,
    /// `FUNC_SIN` ("Sin")
    Sin,
    /// `FUNC_COS` ("Cos")
    Cos,
    /// `FUNC_SIN256` ("Sin256")
    Sin256,
    /// `FUNC_COS256` ("Cos256")
    Cos256,
    /// `FUNC_ATAN2` ("ATan2")
    ATan2,
    /// `FUNC_INTERPOLATE` ("Interpolate")
    Interpolate,
    /// `FUNC_INTERPOLATEXY` ("InterpolateXY")
    InterpolateXY,
    /// `FUNC_LOADSPRITESHEET` ("LoadSpriteSheet")
    LoadSpriteSheet,
    /// `FUNC_REMOVESPRITESHEET` ("RemoveSpriteSheet")
    RemoveSpriteSheet,
    /// `FUNC_DRAWSPRITE` ("DrawSprite")
    DrawSprite,
    /// `FUNC_DRAWSPRITEXY` ("DrawSpriteXY")
    DrawSpriteXY,
    /// `FUNC_DRAWSPRITESCREENXY` ("DrawSpriteScreenXY")
    DrawSpriteScreenXY,
    /// `FUNC_DRAWTINTRECT` ("DrawTintRect")
    DrawTintRect,
    /// `FUNC_DRAWNUMBERS` ("DrawNumbers")
    DrawNumbers,
    /// `FUNC_DRAWACTNAME` ("DrawActName")
    DrawActName,
    /// `FUNC_DRAWMENU` ("DrawMenu")
    DrawMenu,
    /// `FUNC_SPRITEFRAME` ("SpriteFrame")
    SpriteFrame,
    /// `FUNC_EDITFRAME` ("EditFrame")
    EditFrame,
    /// `FUNC_LOADPALETTE` ("LoadPalette")
    LoadPalette,
    /// `FUNC_ROTATEPALETTE` ("RotatePalette")
    RotatePalette,
    /// `FUNC_SETSCREENFADE` ("SetScreenFade")
    SetScreenFade,
    /// `FUNC_SETACTIVEPALETTE` ("SetActivePalette")
    SetActivePalette,
    /// `FUNC_SETPALETTEFADE` ("SetPaletteFade")
    SetPaletteFade,
    /// `FUNC_SETPALETTEENTRY` ("SetPaletteEntry")
    SetPaletteEntry,
    /// `FUNC_GETPALETTEENTRY` ("GetPaletteEntry")
    GetPaletteEntry,
    /// `FUNC_COPYPALETTE` ("CopyPalette")
    CopyPalette,
    /// `FUNC_CLEARSCREEN` ("ClearScreen")
    ClearScreen,
    /// `FUNC_DRAWSPRITEFX` ("DrawSpriteFX")
    DrawSpriteFX,
    /// `FUNC_DRAWSPRITESCREENFX` ("DrawSpriteScreenFX")
    DrawSpriteScreenFX,
    /// `FUNC_LOADANIMATION` ("LoadAnimation")
    LoadAnimation,
    /// `FUNC_SETUPMENU` ("SetupMenu")
    SetupMenu,
    /// `FUNC_ADDMENUENTRY` ("AddMenuEntry")
    AddMenuEntry,
    /// `FUNC_EDITMENUENTRY` ("EditMenuEntry")
    EditMenuEntry,
    /// `FUNC_LOADSTAGE` ("LoadStage")
    LoadStage,
    /// `FUNC_DRAWRECT` ("DrawRect")
    DrawRect,
    /// `FUNC_RESETOBJECTENTITY` ("ResetObjectEntity")
    ResetObjectEntity,
    /// `FUNC_BOXCOLLISIONTEST` ("BoxCollisionTest")
    BoxCollisionTest,
    /// `FUNC_CREATETEMPOBJECT` ("CreateTempObject")
    CreateTempObject,
    /// `FUNC_PROCESSOBJECTMOVEMENT` ("ProcessObjectMovement")
    ProcessObjectMovement,
    /// `FUNC_PROCESSOBJECTCONTROL` ("ProcessObjectControl")
    ProcessObjectControl,
    /// `FUNC_PROCESSANIMATION` ("ProcessAnimation")
    ProcessAnimation,
    /// `FUNC_DRAWOBJECTANIMATION` ("DrawObjectAnimation")
    DrawObjectAnimation,
    /// `FUNC_SETMUSICTRACK` ("SetMusicTrack")
    SetMusicTrack,
    /// `FUNC_PLAYMUSIC` ("PlayMusic")
    PlayMusic,
    /// `FUNC_STOPMUSIC` ("StopMusic")
    StopMusic,
    /// `FUNC_PAUSEMUSIC` ("PauseMusic")
    PauseMusic,
    /// `FUNC_RESUMEMUSIC` ("ResumeMusic")
    ResumeMusic,
    /// `FUNC_SWAPMUSICTRACK` ("SwapMusicTrack")
    SwapMusicTrack,
    /// `FUNC_PLAYSFX` ("PlaySfx")
    PlaySfx,
    /// `FUNC_STOPSFX` ("StopSfx")
    StopSfx,
    /// `FUNC_SETSFXATTRIBUTES` ("SetSfxAttributes")
    SetSfxAttributes,
    /// `FUNC_OBJECTTILECOLLISION` ("ObjectTileCollision")
    ObjectTileCollision,
    /// `FUNC_OBJECTTILEGRIP` ("ObjectTileGrip")
    ObjectTileGrip,
    /// `FUNC_NOT` ("Not")
    Not,
    /// `FUNC_DRAW3DSCENE` ("Draw3DScene")
    Draw3DScene,
    /// `FUNC_SETIDENTITYMATRIX` ("SetIdentityMatrix")
    SetIdentityMatrix,
    /// `FUNC_MATRIXMULTIPLY` ("MatrixMultiply")
    MatrixMultiply,
    /// `FUNC_MATRIXTRANSLATEXYZ` ("MatrixTranslateXYZ")
    MatrixTranslateXYZ,
    /// `FUNC_MATRIXSCALEXYZ` ("MatrixScaleXYZ")
    MatrixScaleXYZ,
    /// `FUNC_MATRIXROTATEX` ("MatrixRotateX")
    MatrixRotateX,
    /// `FUNC_MATRIXROTATEY` ("MatrixRotateY")
    MatrixRotateY,
    /// `FUNC_MATRIXROTATEZ` ("MatrixRotateZ")
    MatrixRotateZ,
    /// `FUNC_MATRIXROTATEXYZ` ("MatrixRotateXYZ")
    MatrixRotateXYZ,
    /// `FUNC_TRANSFORMVERTICES` ("TransformVertices")
    TransformVertices,
    /// `FUNC_CALLFUNCTION` ("CallFunction")
    CallFunction,
    /// `FUNC_RETURN` ("return")
    Return,
    /// `FUNC_SETLAYERDEFORMATION` ("SetLayerDeformation")
    SetLayerDeformation,
    /// `FUNC_CHECKTOUCHRECT` ("CheckTouchRect")
    CheckTouchRect,
    /// `FUNC_GETTILELAYERENTRY` ("GetTileLayerEntry")
    GetTileLayerEntry,
    /// `FUNC_SETTILELAYERENTRY` ("SetTileLayerEntry")
    SetTileLayerEntry,
    /// `FUNC_GETBIT` ("GetBit")
    GetBit,
    /// `FUNC_SETBIT` ("SetBit")
    SetBit,
    /// `FUNC_CLEARDRAWLIST` ("ClearDrawList")
    ClearDrawList,
    /// `FUNC_ADDDRAWLISTENTITYREF` ("AddDrawListEntityRef")
    AddDrawListEntityRef,
    /// `FUNC_GETDRAWLISTENTITYREF` ("GetDrawListEntityRef")
    GetDrawListEntityRef,
    /// `FUNC_SETDRAWLISTENTITYREF` ("SetDrawListEntityRef")
    SetDrawListEntityRef,
    /// `FUNC_GET16X16TILEINFO` ("Get16x16TileInfo")
    Get16x16TileInfo,
    /// `FUNC_SET16X16TILEINFO` ("Set16x16TileInfo")
    Set16x16TileInfo,
    /// `FUNC_COPY16X16TILE` ("Copy16x16Tile")
    Copy16x16Tile,
    /// `FUNC_GETANIMATIONBYNAME` ("GetAnimationByName")
    GetAnimationByName,
    /// `FUNC_READSAVERAM` ("ReadSaveRAM")
    ReadSaveRAM,
    /// `FUNC_WRITESAVERAM` ("WriteSaveRAM")
    WriteSaveRAM,
    /// `FUNC_LOADTEXTFONT` ("LoadFontFile")
    LoadFontFile,
    /// `FUNC_LOADTEXTFILE` ("LoadTextFile")
    LoadTextFile,
    /// `FUNC_GETTEXTINFO` ("GetTextInfo")
    GetTextInfo,
    /// `FUNC_DRAWTEXT` ("DrawText")
    DrawText,
    /// `FUNC_GETVERSIONNUMBER` ("GetVersionNumber")
    GetVersionNumber,
    /// `FUNC_GETTABLEVALUE` ("GetTableValue")
    GetTableValue,
    /// `FUNC_SETTABLEVALUE` ("SetTableValue")
    SetTableValue,
    /// `FUNC_CHECKCURRENTSTAGEFOLDER` ("CheckCurrentStageFolder")
    CheckCurrentStageFolder,
    /// `FUNC_ABS` ("Abs")
    Abs,
    /// `FUNC_CALLNATIVEFUNCTION` ("CallNativeFunction")
    CallNativeFunction,
    /// `FUNC_CALLNATIVEFUNCTION2` ("CallNativeFunction2")
    CallNativeFunction2,
    /// `FUNC_CALLNATIVEFUNCTION4` ("CallNativeFunction4")
    CallNativeFunction4,
    /// `FUNC_SETOBJECTRANGE` ("SetObjectRange")
    SetObjectRange,
    /// `FUNC_PRINT` ("Print")
    Print,
    /// `FUNC_MATRIXINVERSE` ("MatrixInverse")
    MatrixInverse,
    /// `FUNC_GETOBJECTVALUE` ("GetObjectValue")
    GetObjectValue,
    /// `FUNC_SETOBJECTVALUE` ("SetObjectValue")
    SetObjectValue,
    /// `FUNC_COPYOBJECT` ("CopyObject")
    CopyObject,
    /// `FUNC_CHECKCAMERAPROXIMITY` ("CheckCameraProximity")
    CheckCameraProximity,
    /// `FUNC_SETSCREENCOUNT` ("SetScreenCount")
    SetScreenCount,
    /// `FUNC_SETSCREENVERTICES` ("SetScreenVertices")
    SetScreenVertices,
    /// `FUNC_GETINPUTDEVICEID` ("GetInputDeviceID")
    GetInputDeviceID,
    /// `FUNC_GETFILTEREDINPUTDEVICEID` ("GetFilteredInputDeviceID")
    GetFilteredInputDeviceID,
    /// `FUNC_GETINPUTDEVICETYPE` ("GetInputDeviceType")
    GetInputDeviceType,
    /// `FUNC_ISINPUTDEVICEASSIGNED` ("IsInputDeviceAssigned")
    IsInputDeviceAssigned,
    /// `FUNC_ASSIGNINPUTSLOTTODEVICE` ("AssignInputSlotToDevice")
    AssignInputSlotToDevice,
    /// `FUNC_ISSLOTASSIGNED` ("IsInputSlotAssigned")
    IsInputSlotAssigned,
    /// `FUNC_RESETINPUTSLOTASSIGNMENTS` ("ResetInputSlotAssignments")
    ResetInputSlotAssignments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OperandKind {
    SByte,
    Byte,
    SWord,
    Word,
    SInt,
    UInt,
    String,
    Array,
    Function,
    JumpTable,
}

/// Static description of one encoded opcode slot.
///
/// For v4 every operand is a dynamically typed script value (`ScriptVarTypes` in upstream)
/// whose encoding is decided per instruction; `operands` therefore carries one
/// [`OperandKind::SInt`] per upstream `opcodeSize` and only its *length* is meaningful.
/// The richer kinds exist for later script versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OpInfo {
    pub op: Op,
    pub name: &'static str,
    pub operands: &'static [OperandKind],
}

const OP1: &[OperandKind] = &[OperandKind::SInt];

const OP2: &[OperandKind] = &[OperandKind::SInt, OperandKind::SInt];

const OP3: &[OperandKind] = &[OperandKind::SInt, OperandKind::SInt, OperandKind::SInt];

const OP4: &[OperandKind] = &[
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
];

const OP5: &[OperandKind] = &[
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
];

const OP6: &[OperandKind] = &[
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
];

const OP7: &[OperandKind] = &[
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
];

const OP8: &[OperandKind] = &[
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
];

const OP11: &[OperandKind] = &[
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
    OperandKind::SInt,
];

// Revision gating, ported from the `#if` blocks around `enum ScrFunc` and `FunctionInfo
// functions[]`:
//
// * rev00 (early Sonic 1): no `MatrixInverse`; has `LoadFontFile` and `DrawText`; no
//   `GetObjectValue`/`SetObjectValue`/`CopyObject`; no Origins extras; `SetPaletteFade` has
//   7 operands and `LoadTextFile` has 3.
// * rev01 (early Sonic 2): adds `MatrixInverse`, keeps `LoadFontFile`/`DrawText`; still no
//   object-value ops or Origins extras.
// * rev02 (S3&K POC / Sega Forever): drops `LoadFontFile` and `DrawText` (which shifts every
//   later opcode down by two), adds `GetObjectValue`/`SetObjectValue`/`CopyObject`, and reduces
//   `SetPaletteFade` to 6 and `LoadTextFile` to 2 operands.
// * rev03 (Sonic Origins): rev02 plus the ten screen/input operations at the end of the enum
//   (`CheckCameraProximity` .. `ResetInputSlotAssignments`).
//
// Because gated entries shift all following encoded indices, the per-revision tables below are
// the source of truth for decoding; the canonical `Op` enum is revision-independent.

/// Revision rev00 encoded opcode table; index = encoded byte.
static REV00: &[OpInfo] = &[
    OpInfo {
        op: Op::End,
        name: "End",
        operands: &[],
    },
    OpInfo {
        op: Op::Equal,
        name: "Equal",
        operands: OP2,
    },
    OpInfo {
        op: Op::Add,
        name: "Add",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sub,
        name: "Sub",
        operands: OP2,
    },
    OpInfo {
        op: Op::Inc,
        name: "Inc",
        operands: OP1,
    },
    OpInfo {
        op: Op::Dec,
        name: "Dec",
        operands: OP1,
    },
    OpInfo {
        op: Op::Mul,
        name: "Mul",
        operands: OP2,
    },
    OpInfo {
        op: Op::Div,
        name: "Div",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShR,
        name: "ShR",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShL,
        name: "ShL",
        operands: OP2,
    },
    OpInfo {
        op: Op::And,
        name: "And",
        operands: OP2,
    },
    OpInfo {
        op: Op::Or,
        name: "Or",
        operands: OP2,
    },
    OpInfo {
        op: Op::Xor,
        name: "Xor",
        operands: OP2,
    },
    OpInfo {
        op: Op::Mod,
        name: "Mod",
        operands: OP2,
    },
    OpInfo {
        op: Op::FlipSign,
        name: "FlipSign",
        operands: OP1,
    },
    OpInfo {
        op: Op::CheckEqual,
        name: "CheckEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckGreater,
        name: "CheckGreater",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckLower,
        name: "CheckLower",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckNotEqual,
        name: "CheckNotEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::IfEqual,
        name: "IfEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreater,
        name: "IfGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreaterOrEqual,
        name: "IfGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLower,
        name: "IfLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLowerOrEqual,
        name: "IfLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfNotEqual,
        name: "IfNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Else,
        name: "else",
        operands: &[],
    },
    OpInfo {
        op: Op::EndIf,
        name: "endif",
        operands: &[],
    },
    OpInfo {
        op: Op::WEqual,
        name: "WEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreater,
        name: "WGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreaterOrEqual,
        name: "WGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLower,
        name: "WLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLowerOrEqual,
        name: "WLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WNotEqual,
        name: "WNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Loop,
        name: "loop",
        operands: &[],
    },
    OpInfo {
        op: Op::ForEachActive,
        name: "ForEachActive",
        operands: OP3,
    },
    OpInfo {
        op: Op::ForEachAll,
        name: "ForEachAll",
        operands: OP3,
    },
    OpInfo {
        op: Op::Next,
        name: "next",
        operands: &[],
    },
    OpInfo {
        op: Op::Switch,
        name: "switch",
        operands: OP2,
    },
    OpInfo {
        op: Op::Break,
        name: "break",
        operands: &[],
    },
    OpInfo {
        op: Op::EndSwitch,
        name: "endswitch",
        operands: &[],
    },
    OpInfo {
        op: Op::Rand,
        name: "Rand",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin,
        name: "Sin",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos,
        name: "Cos",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin256,
        name: "Sin256",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos256,
        name: "Cos256",
        operands: OP2,
    },
    OpInfo {
        op: Op::ATan2,
        name: "ATan2",
        operands: OP3,
    },
    OpInfo {
        op: Op::Interpolate,
        name: "Interpolate",
        operands: OP4,
    },
    OpInfo {
        op: Op::InterpolateXY,
        name: "InterpolateXY",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadSpriteSheet,
        name: "LoadSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::RemoveSpriteSheet,
        name: "RemoveSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSprite,
        name: "DrawSprite",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteXY,
        name: "DrawSpriteXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawSpriteScreenXY,
        name: "DrawSpriteScreenXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawTintRect,
        name: "DrawTintRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawNumbers,
        name: "DrawNumbers",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawActName,
        name: "DrawActName",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawMenu,
        name: "DrawMenu",
        operands: OP3,
    },
    OpInfo {
        op: Op::SpriteFrame,
        name: "SpriteFrame",
        operands: OP6,
    },
    OpInfo {
        op: Op::EditFrame,
        name: "EditFrame",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadPalette,
        name: "LoadPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::RotatePalette,
        name: "RotatePalette",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetScreenFade,
        name: "SetScreenFade",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetActivePalette,
        name: "SetActivePalette",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetPaletteFade,
        name: "SetPaletteFade",
        operands: OP7,
    },
    OpInfo {
        op: Op::SetPaletteEntry,
        name: "SetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::GetPaletteEntry,
        name: "GetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::CopyPalette,
        name: "CopyPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::ClearScreen,
        name: "ClearScreen",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteFX,
        name: "DrawSpriteFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawSpriteScreenFX,
        name: "DrawSpriteScreenFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadAnimation,
        name: "LoadAnimation",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetupMenu,
        name: "SetupMenu",
        operands: OP4,
    },
    OpInfo {
        op: Op::AddMenuEntry,
        name: "AddMenuEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::EditMenuEntry,
        name: "EditMenuEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadStage,
        name: "LoadStage",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawRect,
        name: "DrawRect",
        operands: OP8,
    },
    OpInfo {
        op: Op::ResetObjectEntity,
        name: "ResetObjectEntity",
        operands: OP5,
    },
    OpInfo {
        op: Op::BoxCollisionTest,
        name: "BoxCollisionTest",
        operands: OP11,
    },
    OpInfo {
        op: Op::CreateTempObject,
        name: "CreateTempObject",
        operands: OP4,
    },
    OpInfo {
        op: Op::ProcessObjectMovement,
        name: "ProcessObjectMovement",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessObjectControl,
        name: "ProcessObjectControl",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessAnimation,
        name: "ProcessAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawObjectAnimation,
        name: "DrawObjectAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::SetMusicTrack,
        name: "SetMusicTrack",
        operands: OP3,
    },
    OpInfo {
        op: Op::PlayMusic,
        name: "PlayMusic",
        operands: OP1,
    },
    OpInfo {
        op: Op::StopMusic,
        name: "StopMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::PauseMusic,
        name: "PauseMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::ResumeMusic,
        name: "ResumeMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::SwapMusicTrack,
        name: "SwapMusicTrack",
        operands: OP4,
    },
    OpInfo {
        op: Op::PlaySfx,
        name: "PlaySfx",
        operands: OP2,
    },
    OpInfo {
        op: Op::StopSfx,
        name: "StopSfx",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetSfxAttributes,
        name: "SetSfxAttributes",
        operands: OP3,
    },
    OpInfo {
        op: Op::ObjectTileCollision,
        name: "ObjectTileCollision",
        operands: OP4,
    },
    OpInfo {
        op: Op::ObjectTileGrip,
        name: "ObjectTileGrip",
        operands: OP4,
    },
    OpInfo {
        op: Op::Not,
        name: "Not",
        operands: OP1,
    },
    OpInfo {
        op: Op::Draw3DScene,
        name: "Draw3DScene",
        operands: &[],
    },
    OpInfo {
        op: Op::SetIdentityMatrix,
        name: "SetIdentityMatrix",
        operands: OP1,
    },
    OpInfo {
        op: Op::MatrixMultiply,
        name: "MatrixMultiply",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixTranslateXYZ,
        name: "MatrixTranslateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixScaleXYZ,
        name: "MatrixScaleXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixRotateX,
        name: "MatrixRotateX",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateY,
        name: "MatrixRotateY",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateZ,
        name: "MatrixRotateZ",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateXYZ,
        name: "MatrixRotateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::TransformVertices,
        name: "TransformVertices",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallFunction,
        name: "CallFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::Return,
        name: "return",
        operands: &[],
    },
    OpInfo {
        op: Op::SetLayerDeformation,
        name: "SetLayerDeformation",
        operands: OP6,
    },
    OpInfo {
        op: Op::CheckTouchRect,
        name: "CheckTouchRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetTileLayerEntry,
        name: "GetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetTileLayerEntry,
        name: "SetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetBit,
        name: "GetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetBit,
        name: "SetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::ClearDrawList,
        name: "ClearDrawList",
        operands: OP1,
    },
    OpInfo {
        op: Op::AddDrawListEntityRef,
        name: "AddDrawListEntityRef",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetDrawListEntityRef,
        name: "GetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetDrawListEntityRef,
        name: "SetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::Get16x16TileInfo,
        name: "Get16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Set16x16TileInfo,
        name: "Set16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Copy16x16Tile,
        name: "Copy16x16Tile",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetAnimationByName,
        name: "GetAnimationByName",
        operands: OP2,
    },
    OpInfo {
        op: Op::ReadSaveRAM,
        name: "ReadSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::WriteSaveRAM,
        name: "WriteSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::LoadFontFile,
        name: "LoadFontFile",
        operands: OP1,
    },
    OpInfo {
        op: Op::LoadTextFile,
        name: "LoadTextFile",
        operands: OP3,
    },
    OpInfo {
        op: Op::GetTextInfo,
        name: "GetTextInfo",
        operands: OP5,
    },
    OpInfo {
        op: Op::DrawText,
        name: "DrawText",
        operands: OP7,
    },
    OpInfo {
        op: Op::GetVersionNumber,
        name: "GetVersionNumber",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetTableValue,
        name: "GetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetTableValue,
        name: "SetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::CheckCurrentStageFolder,
        name: "CheckCurrentStageFolder",
        operands: OP1,
    },
    OpInfo {
        op: Op::Abs,
        name: "Abs",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction,
        name: "CallNativeFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction2,
        name: "CallNativeFunction2",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallNativeFunction4,
        name: "CallNativeFunction4",
        operands: OP5,
    },
    OpInfo {
        op: Op::SetObjectRange,
        name: "SetObjectRange",
        operands: OP1,
    },
    OpInfo {
        op: Op::Print,
        name: "Print",
        operands: OP3,
    },
];

/// Revision rev01 encoded opcode table; index = encoded byte.
static REV01: &[OpInfo] = &[
    OpInfo {
        op: Op::End,
        name: "End",
        operands: &[],
    },
    OpInfo {
        op: Op::Equal,
        name: "Equal",
        operands: OP2,
    },
    OpInfo {
        op: Op::Add,
        name: "Add",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sub,
        name: "Sub",
        operands: OP2,
    },
    OpInfo {
        op: Op::Inc,
        name: "Inc",
        operands: OP1,
    },
    OpInfo {
        op: Op::Dec,
        name: "Dec",
        operands: OP1,
    },
    OpInfo {
        op: Op::Mul,
        name: "Mul",
        operands: OP2,
    },
    OpInfo {
        op: Op::Div,
        name: "Div",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShR,
        name: "ShR",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShL,
        name: "ShL",
        operands: OP2,
    },
    OpInfo {
        op: Op::And,
        name: "And",
        operands: OP2,
    },
    OpInfo {
        op: Op::Or,
        name: "Or",
        operands: OP2,
    },
    OpInfo {
        op: Op::Xor,
        name: "Xor",
        operands: OP2,
    },
    OpInfo {
        op: Op::Mod,
        name: "Mod",
        operands: OP2,
    },
    OpInfo {
        op: Op::FlipSign,
        name: "FlipSign",
        operands: OP1,
    },
    OpInfo {
        op: Op::CheckEqual,
        name: "CheckEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckGreater,
        name: "CheckGreater",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckLower,
        name: "CheckLower",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckNotEqual,
        name: "CheckNotEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::IfEqual,
        name: "IfEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreater,
        name: "IfGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreaterOrEqual,
        name: "IfGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLower,
        name: "IfLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLowerOrEqual,
        name: "IfLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfNotEqual,
        name: "IfNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Else,
        name: "else",
        operands: &[],
    },
    OpInfo {
        op: Op::EndIf,
        name: "endif",
        operands: &[],
    },
    OpInfo {
        op: Op::WEqual,
        name: "WEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreater,
        name: "WGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreaterOrEqual,
        name: "WGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLower,
        name: "WLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLowerOrEqual,
        name: "WLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WNotEqual,
        name: "WNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Loop,
        name: "loop",
        operands: &[],
    },
    OpInfo {
        op: Op::ForEachActive,
        name: "ForEachActive",
        operands: OP3,
    },
    OpInfo {
        op: Op::ForEachAll,
        name: "ForEachAll",
        operands: OP3,
    },
    OpInfo {
        op: Op::Next,
        name: "next",
        operands: &[],
    },
    OpInfo {
        op: Op::Switch,
        name: "switch",
        operands: OP2,
    },
    OpInfo {
        op: Op::Break,
        name: "break",
        operands: &[],
    },
    OpInfo {
        op: Op::EndSwitch,
        name: "endswitch",
        operands: &[],
    },
    OpInfo {
        op: Op::Rand,
        name: "Rand",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin,
        name: "Sin",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos,
        name: "Cos",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin256,
        name: "Sin256",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos256,
        name: "Cos256",
        operands: OP2,
    },
    OpInfo {
        op: Op::ATan2,
        name: "ATan2",
        operands: OP3,
    },
    OpInfo {
        op: Op::Interpolate,
        name: "Interpolate",
        operands: OP4,
    },
    OpInfo {
        op: Op::InterpolateXY,
        name: "InterpolateXY",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadSpriteSheet,
        name: "LoadSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::RemoveSpriteSheet,
        name: "RemoveSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSprite,
        name: "DrawSprite",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteXY,
        name: "DrawSpriteXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawSpriteScreenXY,
        name: "DrawSpriteScreenXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawTintRect,
        name: "DrawTintRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawNumbers,
        name: "DrawNumbers",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawActName,
        name: "DrawActName",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawMenu,
        name: "DrawMenu",
        operands: OP3,
    },
    OpInfo {
        op: Op::SpriteFrame,
        name: "SpriteFrame",
        operands: OP6,
    },
    OpInfo {
        op: Op::EditFrame,
        name: "EditFrame",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadPalette,
        name: "LoadPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::RotatePalette,
        name: "RotatePalette",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetScreenFade,
        name: "SetScreenFade",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetActivePalette,
        name: "SetActivePalette",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetPaletteFade,
        name: "SetPaletteFade",
        operands: OP6,
    },
    OpInfo {
        op: Op::SetPaletteEntry,
        name: "SetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::GetPaletteEntry,
        name: "GetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::CopyPalette,
        name: "CopyPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::ClearScreen,
        name: "ClearScreen",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteFX,
        name: "DrawSpriteFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawSpriteScreenFX,
        name: "DrawSpriteScreenFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadAnimation,
        name: "LoadAnimation",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetupMenu,
        name: "SetupMenu",
        operands: OP4,
    },
    OpInfo {
        op: Op::AddMenuEntry,
        name: "AddMenuEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::EditMenuEntry,
        name: "EditMenuEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadStage,
        name: "LoadStage",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawRect,
        name: "DrawRect",
        operands: OP8,
    },
    OpInfo {
        op: Op::ResetObjectEntity,
        name: "ResetObjectEntity",
        operands: OP5,
    },
    OpInfo {
        op: Op::BoxCollisionTest,
        name: "BoxCollisionTest",
        operands: OP11,
    },
    OpInfo {
        op: Op::CreateTempObject,
        name: "CreateTempObject",
        operands: OP4,
    },
    OpInfo {
        op: Op::ProcessObjectMovement,
        name: "ProcessObjectMovement",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessObjectControl,
        name: "ProcessObjectControl",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessAnimation,
        name: "ProcessAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawObjectAnimation,
        name: "DrawObjectAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::SetMusicTrack,
        name: "SetMusicTrack",
        operands: OP3,
    },
    OpInfo {
        op: Op::PlayMusic,
        name: "PlayMusic",
        operands: OP1,
    },
    OpInfo {
        op: Op::StopMusic,
        name: "StopMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::PauseMusic,
        name: "PauseMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::ResumeMusic,
        name: "ResumeMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::SwapMusicTrack,
        name: "SwapMusicTrack",
        operands: OP4,
    },
    OpInfo {
        op: Op::PlaySfx,
        name: "PlaySfx",
        operands: OP2,
    },
    OpInfo {
        op: Op::StopSfx,
        name: "StopSfx",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetSfxAttributes,
        name: "SetSfxAttributes",
        operands: OP3,
    },
    OpInfo {
        op: Op::ObjectTileCollision,
        name: "ObjectTileCollision",
        operands: OP4,
    },
    OpInfo {
        op: Op::ObjectTileGrip,
        name: "ObjectTileGrip",
        operands: OP4,
    },
    OpInfo {
        op: Op::Not,
        name: "Not",
        operands: OP1,
    },
    OpInfo {
        op: Op::Draw3DScene,
        name: "Draw3DScene",
        operands: &[],
    },
    OpInfo {
        op: Op::SetIdentityMatrix,
        name: "SetIdentityMatrix",
        operands: OP1,
    },
    OpInfo {
        op: Op::MatrixMultiply,
        name: "MatrixMultiply",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixTranslateXYZ,
        name: "MatrixTranslateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixScaleXYZ,
        name: "MatrixScaleXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixRotateX,
        name: "MatrixRotateX",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateY,
        name: "MatrixRotateY",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateZ,
        name: "MatrixRotateZ",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateXYZ,
        name: "MatrixRotateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixInverse,
        name: "MatrixInverse",
        operands: OP1,
    },
    OpInfo {
        op: Op::TransformVertices,
        name: "TransformVertices",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallFunction,
        name: "CallFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::Return,
        name: "return",
        operands: &[],
    },
    OpInfo {
        op: Op::SetLayerDeformation,
        name: "SetLayerDeformation",
        operands: OP6,
    },
    OpInfo {
        op: Op::CheckTouchRect,
        name: "CheckTouchRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetTileLayerEntry,
        name: "GetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetTileLayerEntry,
        name: "SetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetBit,
        name: "GetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetBit,
        name: "SetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::ClearDrawList,
        name: "ClearDrawList",
        operands: OP1,
    },
    OpInfo {
        op: Op::AddDrawListEntityRef,
        name: "AddDrawListEntityRef",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetDrawListEntityRef,
        name: "GetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetDrawListEntityRef,
        name: "SetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::Get16x16TileInfo,
        name: "Get16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Set16x16TileInfo,
        name: "Set16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Copy16x16Tile,
        name: "Copy16x16Tile",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetAnimationByName,
        name: "GetAnimationByName",
        operands: OP2,
    },
    OpInfo {
        op: Op::ReadSaveRAM,
        name: "ReadSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::WriteSaveRAM,
        name: "WriteSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::LoadFontFile,
        name: "LoadFontFile",
        operands: OP1,
    },
    OpInfo {
        op: Op::LoadTextFile,
        name: "LoadTextFile",
        operands: OP3,
    },
    OpInfo {
        op: Op::GetTextInfo,
        name: "GetTextInfo",
        operands: OP5,
    },
    OpInfo {
        op: Op::DrawText,
        name: "DrawText",
        operands: OP7,
    },
    OpInfo {
        op: Op::GetVersionNumber,
        name: "GetVersionNumber",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetTableValue,
        name: "GetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetTableValue,
        name: "SetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::CheckCurrentStageFolder,
        name: "CheckCurrentStageFolder",
        operands: OP1,
    },
    OpInfo {
        op: Op::Abs,
        name: "Abs",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction,
        name: "CallNativeFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction2,
        name: "CallNativeFunction2",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallNativeFunction4,
        name: "CallNativeFunction4",
        operands: OP5,
    },
    OpInfo {
        op: Op::SetObjectRange,
        name: "SetObjectRange",
        operands: OP1,
    },
    OpInfo {
        op: Op::Print,
        name: "Print",
        operands: OP3,
    },
];

/// Revision rev02 encoded opcode table; index = encoded byte.
static REV02: &[OpInfo] = &[
    OpInfo {
        op: Op::End,
        name: "End",
        operands: &[],
    },
    OpInfo {
        op: Op::Equal,
        name: "Equal",
        operands: OP2,
    },
    OpInfo {
        op: Op::Add,
        name: "Add",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sub,
        name: "Sub",
        operands: OP2,
    },
    OpInfo {
        op: Op::Inc,
        name: "Inc",
        operands: OP1,
    },
    OpInfo {
        op: Op::Dec,
        name: "Dec",
        operands: OP1,
    },
    OpInfo {
        op: Op::Mul,
        name: "Mul",
        operands: OP2,
    },
    OpInfo {
        op: Op::Div,
        name: "Div",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShR,
        name: "ShR",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShL,
        name: "ShL",
        operands: OP2,
    },
    OpInfo {
        op: Op::And,
        name: "And",
        operands: OP2,
    },
    OpInfo {
        op: Op::Or,
        name: "Or",
        operands: OP2,
    },
    OpInfo {
        op: Op::Xor,
        name: "Xor",
        operands: OP2,
    },
    OpInfo {
        op: Op::Mod,
        name: "Mod",
        operands: OP2,
    },
    OpInfo {
        op: Op::FlipSign,
        name: "FlipSign",
        operands: OP1,
    },
    OpInfo {
        op: Op::CheckEqual,
        name: "CheckEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckGreater,
        name: "CheckGreater",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckLower,
        name: "CheckLower",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckNotEqual,
        name: "CheckNotEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::IfEqual,
        name: "IfEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreater,
        name: "IfGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreaterOrEqual,
        name: "IfGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLower,
        name: "IfLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLowerOrEqual,
        name: "IfLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfNotEqual,
        name: "IfNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Else,
        name: "else",
        operands: &[],
    },
    OpInfo {
        op: Op::EndIf,
        name: "endif",
        operands: &[],
    },
    OpInfo {
        op: Op::WEqual,
        name: "WEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreater,
        name: "WGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreaterOrEqual,
        name: "WGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLower,
        name: "WLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLowerOrEqual,
        name: "WLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WNotEqual,
        name: "WNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Loop,
        name: "loop",
        operands: &[],
    },
    OpInfo {
        op: Op::ForEachActive,
        name: "ForEachActive",
        operands: OP3,
    },
    OpInfo {
        op: Op::ForEachAll,
        name: "ForEachAll",
        operands: OP3,
    },
    OpInfo {
        op: Op::Next,
        name: "next",
        operands: &[],
    },
    OpInfo {
        op: Op::Switch,
        name: "switch",
        operands: OP2,
    },
    OpInfo {
        op: Op::Break,
        name: "break",
        operands: &[],
    },
    OpInfo {
        op: Op::EndSwitch,
        name: "endswitch",
        operands: &[],
    },
    OpInfo {
        op: Op::Rand,
        name: "Rand",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin,
        name: "Sin",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos,
        name: "Cos",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin256,
        name: "Sin256",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos256,
        name: "Cos256",
        operands: OP2,
    },
    OpInfo {
        op: Op::ATan2,
        name: "ATan2",
        operands: OP3,
    },
    OpInfo {
        op: Op::Interpolate,
        name: "Interpolate",
        operands: OP4,
    },
    OpInfo {
        op: Op::InterpolateXY,
        name: "InterpolateXY",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadSpriteSheet,
        name: "LoadSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::RemoveSpriteSheet,
        name: "RemoveSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSprite,
        name: "DrawSprite",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteXY,
        name: "DrawSpriteXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawSpriteScreenXY,
        name: "DrawSpriteScreenXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawTintRect,
        name: "DrawTintRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawNumbers,
        name: "DrawNumbers",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawActName,
        name: "DrawActName",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawMenu,
        name: "DrawMenu",
        operands: OP3,
    },
    OpInfo {
        op: Op::SpriteFrame,
        name: "SpriteFrame",
        operands: OP6,
    },
    OpInfo {
        op: Op::EditFrame,
        name: "EditFrame",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadPalette,
        name: "LoadPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::RotatePalette,
        name: "RotatePalette",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetScreenFade,
        name: "SetScreenFade",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetActivePalette,
        name: "SetActivePalette",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetPaletteFade,
        name: "SetPaletteFade",
        operands: OP6,
    },
    OpInfo {
        op: Op::SetPaletteEntry,
        name: "SetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::GetPaletteEntry,
        name: "GetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::CopyPalette,
        name: "CopyPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::ClearScreen,
        name: "ClearScreen",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteFX,
        name: "DrawSpriteFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawSpriteScreenFX,
        name: "DrawSpriteScreenFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadAnimation,
        name: "LoadAnimation",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetupMenu,
        name: "SetupMenu",
        operands: OP4,
    },
    OpInfo {
        op: Op::AddMenuEntry,
        name: "AddMenuEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::EditMenuEntry,
        name: "EditMenuEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadStage,
        name: "LoadStage",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawRect,
        name: "DrawRect",
        operands: OP8,
    },
    OpInfo {
        op: Op::ResetObjectEntity,
        name: "ResetObjectEntity",
        operands: OP5,
    },
    OpInfo {
        op: Op::BoxCollisionTest,
        name: "BoxCollisionTest",
        operands: OP11,
    },
    OpInfo {
        op: Op::CreateTempObject,
        name: "CreateTempObject",
        operands: OP4,
    },
    OpInfo {
        op: Op::ProcessObjectMovement,
        name: "ProcessObjectMovement",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessObjectControl,
        name: "ProcessObjectControl",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessAnimation,
        name: "ProcessAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawObjectAnimation,
        name: "DrawObjectAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::SetMusicTrack,
        name: "SetMusicTrack",
        operands: OP3,
    },
    OpInfo {
        op: Op::PlayMusic,
        name: "PlayMusic",
        operands: OP1,
    },
    OpInfo {
        op: Op::StopMusic,
        name: "StopMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::PauseMusic,
        name: "PauseMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::ResumeMusic,
        name: "ResumeMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::SwapMusicTrack,
        name: "SwapMusicTrack",
        operands: OP4,
    },
    OpInfo {
        op: Op::PlaySfx,
        name: "PlaySfx",
        operands: OP2,
    },
    OpInfo {
        op: Op::StopSfx,
        name: "StopSfx",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetSfxAttributes,
        name: "SetSfxAttributes",
        operands: OP3,
    },
    OpInfo {
        op: Op::ObjectTileCollision,
        name: "ObjectTileCollision",
        operands: OP4,
    },
    OpInfo {
        op: Op::ObjectTileGrip,
        name: "ObjectTileGrip",
        operands: OP4,
    },
    OpInfo {
        op: Op::Not,
        name: "Not",
        operands: OP1,
    },
    OpInfo {
        op: Op::Draw3DScene,
        name: "Draw3DScene",
        operands: &[],
    },
    OpInfo {
        op: Op::SetIdentityMatrix,
        name: "SetIdentityMatrix",
        operands: OP1,
    },
    OpInfo {
        op: Op::MatrixMultiply,
        name: "MatrixMultiply",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixTranslateXYZ,
        name: "MatrixTranslateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixScaleXYZ,
        name: "MatrixScaleXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixRotateX,
        name: "MatrixRotateX",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateY,
        name: "MatrixRotateY",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateZ,
        name: "MatrixRotateZ",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateXYZ,
        name: "MatrixRotateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixInverse,
        name: "MatrixInverse",
        operands: OP1,
    },
    OpInfo {
        op: Op::TransformVertices,
        name: "TransformVertices",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallFunction,
        name: "CallFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::Return,
        name: "return",
        operands: &[],
    },
    OpInfo {
        op: Op::SetLayerDeformation,
        name: "SetLayerDeformation",
        operands: OP6,
    },
    OpInfo {
        op: Op::CheckTouchRect,
        name: "CheckTouchRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetTileLayerEntry,
        name: "GetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetTileLayerEntry,
        name: "SetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetBit,
        name: "GetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetBit,
        name: "SetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::ClearDrawList,
        name: "ClearDrawList",
        operands: OP1,
    },
    OpInfo {
        op: Op::AddDrawListEntityRef,
        name: "AddDrawListEntityRef",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetDrawListEntityRef,
        name: "GetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetDrawListEntityRef,
        name: "SetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::Get16x16TileInfo,
        name: "Get16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Set16x16TileInfo,
        name: "Set16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Copy16x16Tile,
        name: "Copy16x16Tile",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetAnimationByName,
        name: "GetAnimationByName",
        operands: OP2,
    },
    OpInfo {
        op: Op::ReadSaveRAM,
        name: "ReadSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::WriteSaveRAM,
        name: "WriteSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::LoadTextFile,
        name: "LoadTextFile",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetTextInfo,
        name: "GetTextInfo",
        operands: OP5,
    },
    OpInfo {
        op: Op::GetVersionNumber,
        name: "GetVersionNumber",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetTableValue,
        name: "GetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetTableValue,
        name: "SetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::CheckCurrentStageFolder,
        name: "CheckCurrentStageFolder",
        operands: OP1,
    },
    OpInfo {
        op: Op::Abs,
        name: "Abs",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction,
        name: "CallNativeFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction2,
        name: "CallNativeFunction2",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallNativeFunction4,
        name: "CallNativeFunction4",
        operands: OP5,
    },
    OpInfo {
        op: Op::SetObjectRange,
        name: "SetObjectRange",
        operands: OP1,
    },
    OpInfo {
        op: Op::GetObjectValue,
        name: "GetObjectValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetObjectValue,
        name: "SetObjectValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::CopyObject,
        name: "CopyObject",
        operands: OP3,
    },
    OpInfo {
        op: Op::Print,
        name: "Print",
        operands: OP3,
    },
];

/// Revision rev03 encoded opcode table; index = encoded byte.
static REV03: &[OpInfo] = &[
    OpInfo {
        op: Op::End,
        name: "End",
        operands: &[],
    },
    OpInfo {
        op: Op::Equal,
        name: "Equal",
        operands: OP2,
    },
    OpInfo {
        op: Op::Add,
        name: "Add",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sub,
        name: "Sub",
        operands: OP2,
    },
    OpInfo {
        op: Op::Inc,
        name: "Inc",
        operands: OP1,
    },
    OpInfo {
        op: Op::Dec,
        name: "Dec",
        operands: OP1,
    },
    OpInfo {
        op: Op::Mul,
        name: "Mul",
        operands: OP2,
    },
    OpInfo {
        op: Op::Div,
        name: "Div",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShR,
        name: "ShR",
        operands: OP2,
    },
    OpInfo {
        op: Op::ShL,
        name: "ShL",
        operands: OP2,
    },
    OpInfo {
        op: Op::And,
        name: "And",
        operands: OP2,
    },
    OpInfo {
        op: Op::Or,
        name: "Or",
        operands: OP2,
    },
    OpInfo {
        op: Op::Xor,
        name: "Xor",
        operands: OP2,
    },
    OpInfo {
        op: Op::Mod,
        name: "Mod",
        operands: OP2,
    },
    OpInfo {
        op: Op::FlipSign,
        name: "FlipSign",
        operands: OP1,
    },
    OpInfo {
        op: Op::CheckEqual,
        name: "CheckEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckGreater,
        name: "CheckGreater",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckLower,
        name: "CheckLower",
        operands: OP2,
    },
    OpInfo {
        op: Op::CheckNotEqual,
        name: "CheckNotEqual",
        operands: OP2,
    },
    OpInfo {
        op: Op::IfEqual,
        name: "IfEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreater,
        name: "IfGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfGreaterOrEqual,
        name: "IfGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLower,
        name: "IfLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfLowerOrEqual,
        name: "IfLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::IfNotEqual,
        name: "IfNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Else,
        name: "else",
        operands: &[],
    },
    OpInfo {
        op: Op::EndIf,
        name: "endif",
        operands: &[],
    },
    OpInfo {
        op: Op::WEqual,
        name: "WEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreater,
        name: "WGreater",
        operands: OP3,
    },
    OpInfo {
        op: Op::WGreaterOrEqual,
        name: "WGreaterOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLower,
        name: "WLower",
        operands: OP3,
    },
    OpInfo {
        op: Op::WLowerOrEqual,
        name: "WLowerOrEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::WNotEqual,
        name: "WNotEqual",
        operands: OP3,
    },
    OpInfo {
        op: Op::Loop,
        name: "loop",
        operands: &[],
    },
    OpInfo {
        op: Op::ForEachActive,
        name: "ForEachActive",
        operands: OP3,
    },
    OpInfo {
        op: Op::ForEachAll,
        name: "ForEachAll",
        operands: OP3,
    },
    OpInfo {
        op: Op::Next,
        name: "next",
        operands: &[],
    },
    OpInfo {
        op: Op::Switch,
        name: "switch",
        operands: OP2,
    },
    OpInfo {
        op: Op::Break,
        name: "break",
        operands: &[],
    },
    OpInfo {
        op: Op::EndSwitch,
        name: "endswitch",
        operands: &[],
    },
    OpInfo {
        op: Op::Rand,
        name: "Rand",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin,
        name: "Sin",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos,
        name: "Cos",
        operands: OP2,
    },
    OpInfo {
        op: Op::Sin256,
        name: "Sin256",
        operands: OP2,
    },
    OpInfo {
        op: Op::Cos256,
        name: "Cos256",
        operands: OP2,
    },
    OpInfo {
        op: Op::ATan2,
        name: "ATan2",
        operands: OP3,
    },
    OpInfo {
        op: Op::Interpolate,
        name: "Interpolate",
        operands: OP4,
    },
    OpInfo {
        op: Op::InterpolateXY,
        name: "InterpolateXY",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadSpriteSheet,
        name: "LoadSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::RemoveSpriteSheet,
        name: "RemoveSpriteSheet",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSprite,
        name: "DrawSprite",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteXY,
        name: "DrawSpriteXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawSpriteScreenXY,
        name: "DrawSpriteScreenXY",
        operands: OP3,
    },
    OpInfo {
        op: Op::DrawTintRect,
        name: "DrawTintRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawNumbers,
        name: "DrawNumbers",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawActName,
        name: "DrawActName",
        operands: OP7,
    },
    OpInfo {
        op: Op::DrawMenu,
        name: "DrawMenu",
        operands: OP3,
    },
    OpInfo {
        op: Op::SpriteFrame,
        name: "SpriteFrame",
        operands: OP6,
    },
    OpInfo {
        op: Op::EditFrame,
        name: "EditFrame",
        operands: OP7,
    },
    OpInfo {
        op: Op::LoadPalette,
        name: "LoadPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::RotatePalette,
        name: "RotatePalette",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetScreenFade,
        name: "SetScreenFade",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetActivePalette,
        name: "SetActivePalette",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetPaletteFade,
        name: "SetPaletteFade",
        operands: OP6,
    },
    OpInfo {
        op: Op::SetPaletteEntry,
        name: "SetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::GetPaletteEntry,
        name: "GetPaletteEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::CopyPalette,
        name: "CopyPalette",
        operands: OP5,
    },
    OpInfo {
        op: Op::ClearScreen,
        name: "ClearScreen",
        operands: OP1,
    },
    OpInfo {
        op: Op::DrawSpriteFX,
        name: "DrawSpriteFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::DrawSpriteScreenFX,
        name: "DrawSpriteScreenFX",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadAnimation,
        name: "LoadAnimation",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetupMenu,
        name: "SetupMenu",
        operands: OP4,
    },
    OpInfo {
        op: Op::AddMenuEntry,
        name: "AddMenuEntry",
        operands: OP3,
    },
    OpInfo {
        op: Op::EditMenuEntry,
        name: "EditMenuEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::LoadStage,
        name: "LoadStage",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawRect,
        name: "DrawRect",
        operands: OP8,
    },
    OpInfo {
        op: Op::ResetObjectEntity,
        name: "ResetObjectEntity",
        operands: OP5,
    },
    OpInfo {
        op: Op::BoxCollisionTest,
        name: "BoxCollisionTest",
        operands: OP11,
    },
    OpInfo {
        op: Op::CreateTempObject,
        name: "CreateTempObject",
        operands: OP4,
    },
    OpInfo {
        op: Op::ProcessObjectMovement,
        name: "ProcessObjectMovement",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessObjectControl,
        name: "ProcessObjectControl",
        operands: &[],
    },
    OpInfo {
        op: Op::ProcessAnimation,
        name: "ProcessAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::DrawObjectAnimation,
        name: "DrawObjectAnimation",
        operands: &[],
    },
    OpInfo {
        op: Op::SetMusicTrack,
        name: "SetMusicTrack",
        operands: OP3,
    },
    OpInfo {
        op: Op::PlayMusic,
        name: "PlayMusic",
        operands: OP1,
    },
    OpInfo {
        op: Op::StopMusic,
        name: "StopMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::PauseMusic,
        name: "PauseMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::ResumeMusic,
        name: "ResumeMusic",
        operands: &[],
    },
    OpInfo {
        op: Op::SwapMusicTrack,
        name: "SwapMusicTrack",
        operands: OP4,
    },
    OpInfo {
        op: Op::PlaySfx,
        name: "PlaySfx",
        operands: OP2,
    },
    OpInfo {
        op: Op::StopSfx,
        name: "StopSfx",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetSfxAttributes,
        name: "SetSfxAttributes",
        operands: OP3,
    },
    OpInfo {
        op: Op::ObjectTileCollision,
        name: "ObjectTileCollision",
        operands: OP4,
    },
    OpInfo {
        op: Op::ObjectTileGrip,
        name: "ObjectTileGrip",
        operands: OP4,
    },
    OpInfo {
        op: Op::Not,
        name: "Not",
        operands: OP1,
    },
    OpInfo {
        op: Op::Draw3DScene,
        name: "Draw3DScene",
        operands: &[],
    },
    OpInfo {
        op: Op::SetIdentityMatrix,
        name: "SetIdentityMatrix",
        operands: OP1,
    },
    OpInfo {
        op: Op::MatrixMultiply,
        name: "MatrixMultiply",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixTranslateXYZ,
        name: "MatrixTranslateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixScaleXYZ,
        name: "MatrixScaleXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixRotateX,
        name: "MatrixRotateX",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateY,
        name: "MatrixRotateY",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateZ,
        name: "MatrixRotateZ",
        operands: OP2,
    },
    OpInfo {
        op: Op::MatrixRotateXYZ,
        name: "MatrixRotateXYZ",
        operands: OP4,
    },
    OpInfo {
        op: Op::MatrixInverse,
        name: "MatrixInverse",
        operands: OP1,
    },
    OpInfo {
        op: Op::TransformVertices,
        name: "TransformVertices",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallFunction,
        name: "CallFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::Return,
        name: "return",
        operands: &[],
    },
    OpInfo {
        op: Op::SetLayerDeformation,
        name: "SetLayerDeformation",
        operands: OP6,
    },
    OpInfo {
        op: Op::CheckTouchRect,
        name: "CheckTouchRect",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetTileLayerEntry,
        name: "GetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetTileLayerEntry,
        name: "SetTileLayerEntry",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetBit,
        name: "GetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetBit,
        name: "SetBit",
        operands: OP3,
    },
    OpInfo {
        op: Op::ClearDrawList,
        name: "ClearDrawList",
        operands: OP1,
    },
    OpInfo {
        op: Op::AddDrawListEntityRef,
        name: "AddDrawListEntityRef",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetDrawListEntityRef,
        name: "GetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetDrawListEntityRef,
        name: "SetDrawListEntityRef",
        operands: OP3,
    },
    OpInfo {
        op: Op::Get16x16TileInfo,
        name: "Get16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Set16x16TileInfo,
        name: "Set16x16TileInfo",
        operands: OP4,
    },
    OpInfo {
        op: Op::Copy16x16Tile,
        name: "Copy16x16Tile",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetAnimationByName,
        name: "GetAnimationByName",
        operands: OP2,
    },
    OpInfo {
        op: Op::ReadSaveRAM,
        name: "ReadSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::WriteSaveRAM,
        name: "WriteSaveRAM",
        operands: &[],
    },
    OpInfo {
        op: Op::LoadTextFile,
        name: "LoadTextFile",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetTextInfo,
        name: "GetTextInfo",
        operands: OP5,
    },
    OpInfo {
        op: Op::GetVersionNumber,
        name: "GetVersionNumber",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetTableValue,
        name: "GetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetTableValue,
        name: "SetTableValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::CheckCurrentStageFolder,
        name: "CheckCurrentStageFolder",
        operands: OP1,
    },
    OpInfo {
        op: Op::Abs,
        name: "Abs",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction,
        name: "CallNativeFunction",
        operands: OP1,
    },
    OpInfo {
        op: Op::CallNativeFunction2,
        name: "CallNativeFunction2",
        operands: OP3,
    },
    OpInfo {
        op: Op::CallNativeFunction4,
        name: "CallNativeFunction4",
        operands: OP5,
    },
    OpInfo {
        op: Op::SetObjectRange,
        name: "SetObjectRange",
        operands: OP1,
    },
    OpInfo {
        op: Op::GetObjectValue,
        name: "GetObjectValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::SetObjectValue,
        name: "SetObjectValue",
        operands: OP3,
    },
    OpInfo {
        op: Op::CopyObject,
        name: "CopyObject",
        operands: OP3,
    },
    OpInfo {
        op: Op::Print,
        name: "Print",
        operands: OP3,
    },
    OpInfo {
        op: Op::CheckCameraProximity,
        name: "CheckCameraProximity",
        operands: OP4,
    },
    OpInfo {
        op: Op::SetScreenCount,
        name: "SetScreenCount",
        operands: OP1,
    },
    OpInfo {
        op: Op::SetScreenVertices,
        name: "SetScreenVertices",
        operands: OP5,
    },
    OpInfo {
        op: Op::GetInputDeviceID,
        name: "GetInputDeviceID",
        operands: OP2,
    },
    OpInfo {
        op: Op::GetFilteredInputDeviceID,
        name: "GetFilteredInputDeviceID",
        operands: OP4,
    },
    OpInfo {
        op: Op::GetInputDeviceType,
        name: "GetInputDeviceType",
        operands: OP2,
    },
    OpInfo {
        op: Op::IsInputDeviceAssigned,
        name: "IsInputDeviceAssigned",
        operands: OP1,
    },
    OpInfo {
        op: Op::AssignInputSlotToDevice,
        name: "AssignInputSlotToDevice",
        operands: OP2,
    },
    OpInfo {
        op: Op::IsInputSlotAssigned,
        name: "IsInputSlotAssigned",
        operands: OP1,
    },
    OpInfo {
        op: Op::ResetInputSlotAssignments,
        name: "ResetInputSlotAssignments",
        operands: &[],
    },
];

/// Number of encoded opcodes per revision, used by tests and callers that size buffers.
pub const OPCODE_COUNTS: [(V4Revision, usize); 4] = [
    (V4Revision::Rev00, 137),
    (V4Revision::Rev01, 138),
    (V4Revision::Rev02, 139),
    (V4Revision::Rev03, 149),
];

/// Encoded opcode table for a v4 revision; the slice index is the byte stored in bytecode.
pub fn revision_table(revision: V4Revision) -> &'static [OpInfo] {
    match revision {
        V4Revision::Rev00 => REV00,
        V4Revision::Rev01 => REV01,
        V4Revision::Rev02 => REV02,
        V4Revision::Rev03 => REV03,
    }
}

/// Encoded opcode table for a script version.
///
/// Only the v4 tables are ported; [`ScriptVersion::V2`] and [`ScriptVersion::V3`] currently
/// resolve to the v4 table for `revision` so callers get a well-defined table while those VMs
/// remain unported.
pub fn opcode_table(version: ScriptVersion, revision: V4Revision) -> &'static [OpInfo] {
    revision_table(version.v4_revision(revision))
}

/// Decodes an encoded opcode byte into its canonical [`Op`].
pub fn canonical_op(version: ScriptVersion, revision: V4Revision, encoded: u8) -> Option<Op> {
    opcode_table(version, revision)
        .get(encoded as usize)
        .map(|info| info.op)
}

/// Returns the encoded opcode byte for `op` under `revision`, if the operation exists there.
pub fn encoded_opcode(version: ScriptVersion, revision: V4Revision, op: Op) -> Option<u8> {
    opcode_table(version, revision)
        .iter()
        .position(|info| info.op == op)
        .map(|index| index as u8)
}

/// Looks up an encoded opcode by its upstream name (for example `"CallFunction"`, `"else"`).
///
/// Matching is exact and case-sensitive, mirroring upstream `StrComp` in the text compiler.
pub fn opcode_by_name(version: ScriptVersion, revision: V4Revision, name: &str) -> Option<u8> {
    opcode_table(version, revision)
        .iter()
        .position(|info| info.name == name)
        .map(|index| index as u8)
}

/// Returns whether the VM normally writes operand values back to their decoded destinations
/// after `op`.
///
/// Upstream `ProcessScript` skips the "Set Values" pass for control-flow operations (which jump
/// away) and for engine operations that explicitly zero `opcodeSize`. This function encodes that
/// static table; it is revision-independent because the same operation always behaves the same
/// way.
///
/// `ForEachActive`/`ForEachAll` return `true`: upstream leaves `opcodeSize` intact on the
/// success path so the entity reference in `operands[2]` is written to the loop variable, and
/// only suppresses write-back on the exit paths. The VM tracks that per-path suppression
/// internally (see `Vm::foreach_step`).
pub fn op_writes_back(op: Op) -> bool {
    !matches!(
        op,
        Op::CheckEqual
            | Op::CheckGreater
            | Op::CheckLower
            | Op::CheckNotEqual
            | Op::IfEqual
            | Op::IfGreater
            | Op::IfGreaterOrEqual
            | Op::IfLower
            | Op::IfLowerOrEqual
            | Op::IfNotEqual
            | Op::Else
            | Op::EndIf
            | Op::WEqual
            | Op::WGreater
            | Op::WGreaterOrEqual
            | Op::WLower
            | Op::WLowerOrEqual
            | Op::WNotEqual
            | Op::Loop
            | Op::Next
            | Op::Switch
            | Op::Break
            | Op::EndSwitch
            | Op::LoadSpriteSheet
            | Op::RemoveSpriteSheet
            | Op::DrawSprite
            | Op::DrawSpriteXY
            | Op::DrawSpriteScreenXY
            | Op::DrawTintRect
            | Op::DrawNumbers
            | Op::DrawActName
            | Op::DrawMenu
            | Op::SpriteFrame
            | Op::EditFrame
            | Op::LoadPalette
            | Op::RotatePalette
            | Op::SetScreenFade
            | Op::SetActivePalette
            | Op::CopyPalette
            | Op::ClearScreen
            | Op::DrawSpriteFX
            | Op::DrawSpriteScreenFX
            | Op::LoadAnimation
            | Op::SetupMenu
            | Op::AddMenuEntry
            | Op::EditMenuEntry
            | Op::LoadStage
            | Op::DrawRect
            | Op::ResetObjectEntity
            | Op::BoxCollisionTest
            | Op::CreateTempObject
            | Op::ProcessObjectMovement
            | Op::ProcessObjectControl
            | Op::ProcessAnimation
            | Op::DrawObjectAnimation
            | Op::SetMusicTrack
            | Op::PlayMusic
            | Op::StopMusic
            | Op::PauseMusic
            | Op::ResumeMusic
            | Op::SwapMusicTrack
            | Op::PlaySfx
            | Op::StopSfx
            | Op::SetSfxAttributes
            | Op::ObjectTileCollision
            | Op::ObjectTileGrip
            | Op::Draw3DScene
            | Op::SetIdentityMatrix
            | Op::MatrixMultiply
            | Op::MatrixTranslateXYZ
            | Op::MatrixScaleXYZ
            | Op::MatrixRotateX
            | Op::MatrixRotateY
            | Op::MatrixRotateZ
            | Op::MatrixRotateXYZ
            | Op::MatrixInverse
            | Op::TransformVertices
            | Op::CallFunction
            | Op::Return
            | Op::SetLayerDeformation
            | Op::CheckTouchRect
            | Op::ClearDrawList
            | Op::AddDrawListEntityRef
            | Op::SetDrawListEntityRef
            | Op::Copy16x16Tile
            | Op::ReadSaveRAM
            | Op::WriteSaveRAM
            | Op::LoadFontFile
            | Op::LoadTextFile
            | Op::DrawText
            | Op::GetVersionNumber
            | Op::SetTableValue
            | Op::CheckCurrentStageFolder
            | Op::CallNativeFunction
            | Op::SetObjectRange
            | Op::SetObjectValue
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Complete upstream rev03 name sequence (`FunctionInfo` strings in enum order).
    const REV03_NAMES: &[&str] = &[
        "End",
        "Equal",
        "Add",
        "Sub",
        "Inc",
        "Dec",
        "Mul",
        "Div",
        "ShR",
        "ShL",
        "And",
        "Or",
        "Xor",
        "Mod",
        "FlipSign",
        "CheckEqual",
        "CheckGreater",
        "CheckLower",
        "CheckNotEqual",
        "IfEqual",
        "IfGreater",
        "IfGreaterOrEqual",
        "IfLower",
        "IfLowerOrEqual",
        "IfNotEqual",
        "else",
        "endif",
        "WEqual",
        "WGreater",
        "WGreaterOrEqual",
        "WLower",
        "WLowerOrEqual",
        "WNotEqual",
        "loop",
        "ForEachActive",
        "ForEachAll",
        "next",
        "switch",
        "break",
        "endswitch",
        "Rand",
        "Sin",
        "Cos",
        "Sin256",
        "Cos256",
        "ATan2",
        "Interpolate",
        "InterpolateXY",
        "LoadSpriteSheet",
        "RemoveSpriteSheet",
        "DrawSprite",
        "DrawSpriteXY",
        "DrawSpriteScreenXY",
        "DrawTintRect",
        "DrawNumbers",
        "DrawActName",
        "DrawMenu",
        "SpriteFrame",
        "EditFrame",
        "LoadPalette",
        "RotatePalette",
        "SetScreenFade",
        "SetActivePalette",
        "SetPaletteFade",
        "SetPaletteEntry",
        "GetPaletteEntry",
        "CopyPalette",
        "ClearScreen",
        "DrawSpriteFX",
        "DrawSpriteScreenFX",
        "LoadAnimation",
        "SetupMenu",
        "AddMenuEntry",
        "EditMenuEntry",
        "LoadStage",
        "DrawRect",
        "ResetObjectEntity",
        "BoxCollisionTest",
        "CreateTempObject",
        "ProcessObjectMovement",
        "ProcessObjectControl",
        "ProcessAnimation",
        "DrawObjectAnimation",
        "SetMusicTrack",
        "PlayMusic",
        "StopMusic",
        "PauseMusic",
        "ResumeMusic",
        "SwapMusicTrack",
        "PlaySfx",
        "StopSfx",
        "SetSfxAttributes",
        "ObjectTileCollision",
        "ObjectTileGrip",
        "Not",
        "Draw3DScene",
        "SetIdentityMatrix",
        "MatrixMultiply",
        "MatrixTranslateXYZ",
        "MatrixScaleXYZ",
        "MatrixRotateX",
        "MatrixRotateY",
        "MatrixRotateZ",
        "MatrixRotateXYZ",
        "MatrixInverse",
        "TransformVertices",
        "CallFunction",
        "return",
        "SetLayerDeformation",
        "CheckTouchRect",
        "GetTileLayerEntry",
        "SetTileLayerEntry",
        "GetBit",
        "SetBit",
        "ClearDrawList",
        "AddDrawListEntityRef",
        "GetDrawListEntityRef",
        "SetDrawListEntityRef",
        "Get16x16TileInfo",
        "Set16x16TileInfo",
        "Copy16x16Tile",
        "GetAnimationByName",
        "ReadSaveRAM",
        "WriteSaveRAM",
        "LoadTextFile",
        "GetTextInfo",
        "GetVersionNumber",
        "GetTableValue",
        "SetTableValue",
        "CheckCurrentStageFolder",
        "Abs",
        "CallNativeFunction",
        "CallNativeFunction2",
        "CallNativeFunction4",
        "SetObjectRange",
        "GetObjectValue",
        "SetObjectValue",
        "CopyObject",
        "Print",
        "CheckCameraProximity",
        "SetScreenCount",
        "SetScreenVertices",
        "GetInputDeviceID",
        "GetFilteredInputDeviceID",
        "GetInputDeviceType",
        "IsInputDeviceAssigned",
        "AssignInputSlotToDevice",
        "IsInputSlotAssigned",
        "ResetInputSlotAssignments",
    ];

    /// Complete upstream rev00 name sequence; differs by `MatrixInverse`, the two text ops and
    /// the three rev02 object ops.
    const REV00_NAMES: &[&str] = &[
        "End",
        "Equal",
        "Add",
        "Sub",
        "Inc",
        "Dec",
        "Mul",
        "Div",
        "ShR",
        "ShL",
        "And",
        "Or",
        "Xor",
        "Mod",
        "FlipSign",
        "CheckEqual",
        "CheckGreater",
        "CheckLower",
        "CheckNotEqual",
        "IfEqual",
        "IfGreater",
        "IfGreaterOrEqual",
        "IfLower",
        "IfLowerOrEqual",
        "IfNotEqual",
        "else",
        "endif",
        "WEqual",
        "WGreater",
        "WGreaterOrEqual",
        "WLower",
        "WLowerOrEqual",
        "WNotEqual",
        "loop",
        "ForEachActive",
        "ForEachAll",
        "next",
        "switch",
        "break",
        "endswitch",
        "Rand",
        "Sin",
        "Cos",
        "Sin256",
        "Cos256",
        "ATan2",
        "Interpolate",
        "InterpolateXY",
        "LoadSpriteSheet",
        "RemoveSpriteSheet",
        "DrawSprite",
        "DrawSpriteXY",
        "DrawSpriteScreenXY",
        "DrawTintRect",
        "DrawNumbers",
        "DrawActName",
        "DrawMenu",
        "SpriteFrame",
        "EditFrame",
        "LoadPalette",
        "RotatePalette",
        "SetScreenFade",
        "SetActivePalette",
        "SetPaletteFade",
        "SetPaletteEntry",
        "GetPaletteEntry",
        "CopyPalette",
        "ClearScreen",
        "DrawSpriteFX",
        "DrawSpriteScreenFX",
        "LoadAnimation",
        "SetupMenu",
        "AddMenuEntry",
        "EditMenuEntry",
        "LoadStage",
        "DrawRect",
        "ResetObjectEntity",
        "BoxCollisionTest",
        "CreateTempObject",
        "ProcessObjectMovement",
        "ProcessObjectControl",
        "ProcessAnimation",
        "DrawObjectAnimation",
        "SetMusicTrack",
        "PlayMusic",
        "StopMusic",
        "PauseMusic",
        "ResumeMusic",
        "SwapMusicTrack",
        "PlaySfx",
        "StopSfx",
        "SetSfxAttributes",
        "ObjectTileCollision",
        "ObjectTileGrip",
        "Not",
        "Draw3DScene",
        "SetIdentityMatrix",
        "MatrixMultiply",
        "MatrixTranslateXYZ",
        "MatrixScaleXYZ",
        "MatrixRotateX",
        "MatrixRotateY",
        "MatrixRotateZ",
        "MatrixRotateXYZ",
        "TransformVertices",
        "CallFunction",
        "return",
        "SetLayerDeformation",
        "CheckTouchRect",
        "GetTileLayerEntry",
        "SetTileLayerEntry",
        "GetBit",
        "SetBit",
        "ClearDrawList",
        "AddDrawListEntityRef",
        "GetDrawListEntityRef",
        "SetDrawListEntityRef",
        "Get16x16TileInfo",
        "Set16x16TileInfo",
        "Copy16x16Tile",
        "GetAnimationByName",
        "ReadSaveRAM",
        "WriteSaveRAM",
        "LoadFontFile",
        "LoadTextFile",
        "GetTextInfo",
        "DrawText",
        "GetVersionNumber",
        "GetTableValue",
        "SetTableValue",
        "CheckCurrentStageFolder",
        "Abs",
        "CallNativeFunction",
        "CallNativeFunction2",
        "CallNativeFunction4",
        "SetObjectRange",
        "Print",
    ];

    #[test]
    fn counts_match_upstream_gating() {
        assert_eq!(
            opcode_table(ScriptVersion::V4, V4Revision::Rev00).len(),
            137
        );
        assert_eq!(
            opcode_table(ScriptVersion::V4, V4Revision::Rev01).len(),
            138
        );
        assert_eq!(
            opcode_table(ScriptVersion::V4, V4Revision::Rev02).len(),
            139
        );
        assert_eq!(
            opcode_table(ScriptVersion::V4, V4Revision::Rev03).len(),
            149
        );
        for (revision, count) in OPCODE_COUNTS {
            assert_eq!(revision_table(revision).len(), count);
        }
    }

    #[test]
    fn rev03_names_match_upstream_enum_order() {
        let names: Vec<&str> = revision_table(V4Revision::Rev03)
            .iter()
            .map(|info| info.name)
            .collect();
        assert_eq!(names, REV03_NAMES);
    }

    #[test]
    fn rev00_names_match_upstream_enum_order() {
        let names: Vec<&str> = revision_table(V4Revision::Rev00)
            .iter()
            .map(|info| info.name)
            .collect();
        assert_eq!(names, REV00_NAMES);
    }

    #[test]
    fn known_encoded_indices() {
        let v4 = ScriptVersion::V4;
        assert_eq!(canonical_op(v4, V4Revision::Rev03, 0), Some(Op::End));
        assert_eq!(canonical_op(v4, V4Revision::Rev03, 2), Some(Op::Add));
        assert_eq!(
            canonical_op(v4, V4Revision::Rev03, 106),
            Some(Op::CallFunction)
        );
        assert_eq!(
            canonical_op(v4, V4Revision::Rev03, 148),
            Some(Op::ResetInputSlotAssignments)
        );
        // rev00 has no MatrixInverse, so TransformVertices and CallFunction move down one slot.
        assert_eq!(
            canonical_op(v4, V4Revision::Rev00, 104),
            Some(Op::TransformVertices)
        );
        assert_eq!(
            canonical_op(v4, V4Revision::Rev00, 105),
            Some(Op::CallFunction)
        );
        assert_eq!(encoded_opcode(v4, V4Revision::Rev03, Op::End), Some(0));
        assert_eq!(encoded_opcode(v4, V4Revision::Rev03, Op::Add), Some(2));
        assert_eq!(
            encoded_opcode(v4, V4Revision::Rev03, Op::CallFunction),
            Some(106)
        );
        assert_eq!(
            encoded_opcode(v4, V4Revision::Rev00, Op::MatrixInverse),
            None
        );
        assert_eq!(
            encoded_opcode(v4, V4Revision::Rev03, Op::LoadFontFile),
            None
        );
        assert_eq!(encoded_opcode(v4, V4Revision::Rev03, Op::DrawText), None);
        assert_eq!(
            encoded_opcode(v4, V4Revision::Rev02, Op::LoadFontFile),
            None
        );
        assert!(encoded_opcode(v4, V4Revision::Rev01, Op::DrawText).is_some());
        assert_eq!(
            encoded_opcode(v4, V4Revision::Rev00, Op::GetObjectValue),
            None
        );
        assert!(encoded_opcode(v4, V4Revision::Rev02, Op::GetObjectValue).is_some());
    }

    #[test]
    fn origins_only_additions_are_at_the_end() {
        let table = revision_table(V4Revision::Rev03);
        let origins = [
            Op::CheckCameraProximity,
            Op::SetScreenCount,
            Op::SetScreenVertices,
            Op::GetInputDeviceID,
            Op::GetFilteredInputDeviceID,
            Op::GetInputDeviceType,
            Op::IsInputDeviceAssigned,
            Op::AssignInputSlotToDevice,
            Op::IsInputSlotAssigned,
            Op::ResetInputSlotAssignments,
        ];
        for (offset, op) in origins.iter().enumerate() {
            let index = table.len() - origins.len() + offset;
            assert_eq!(table[index].op, *op);
            assert_eq!(
                encoded_opcode(ScriptVersion::V4, V4Revision::Rev03, *op),
                Some(index as u8)
            );
            assert_eq!(
                encoded_opcode(ScriptVersion::V4, V4Revision::Rev02, *op),
                None
            );
        }
    }

    #[test]
    fn every_entry_round_trips_through_its_encoded_byte() {
        for revision in V4Revision::ALL {
            let table = revision_table(revision);
            for (index, info) in table.iter().enumerate() {
                let encoded = index as u8;
                assert_eq!(
                    canonical_op(ScriptVersion::V4, revision, encoded),
                    Some(info.op)
                );
                assert_eq!(
                    encoded_opcode(ScriptVersion::V4, revision, info.op),
                    Some(encoded)
                );
                assert_eq!(
                    opcode_by_name(ScriptVersion::V4, revision, info.name),
                    Some(encoded)
                );
                assert_eq!(
                    opcode_table(ScriptVersion::V4, revision)[encoded as usize].name,
                    info.name
                );
            }
            // Out-of-range bytes decode to None.
            assert_eq!(canonical_op(ScriptVersion::V4, revision, 200), None);
        }
    }

    #[test]
    fn names_are_unique_within_each_revision() {
        for revision in V4Revision::ALL {
            let mut seen = std::collections::HashSet::new();
            for info in revision_table(revision) {
                assert!(
                    seen.insert(info.name),
                    "duplicate opcode name {} in {}",
                    info.name,
                    revision.name()
                );
            }
        }
    }

    #[test]
    fn v2_and_v3_fall_back_to_v4_tables() {
        assert_eq!(
            opcode_table(ScriptVersion::V2, V4Revision::Rev03).as_ptr(),
            opcode_table(ScriptVersion::V4, V4Revision::Rev03).as_ptr()
        );
        assert_eq!(
            opcode_table(ScriptVersion::V3, V4Revision::Rev01).as_ptr(),
            opcode_table(ScriptVersion::V4, V4Revision::Rev01).as_ptr()
        );
    }

    #[test]
    fn operand_lengths_match_upstream_opcode_sizes() {
        // Spot checks across the size spectrum; the generated table is the source of truth.
        let v4 = ScriptVersion::V4;
        let size = |op| {
            revision_table(V4Revision::Rev03)
                .iter()
                .find(|info| info.op == op)
                .unwrap()
                .operands
                .len()
        };
        assert_eq!(size(Op::End), 0);
        assert_eq!(size(Op::Add), 2);
        assert_eq!(size(Op::IfEqual), 3);
        assert_eq!(size(Op::Switch), 2);
        assert_eq!(size(Op::BoxCollisionTest), 11);
        assert_eq!(size(Op::ResetInputSlotAssignments), 0);
        // Revision-gated operand counts.
        let fade_rev00 = revision_table(V4Revision::Rev00)
            .iter()
            .find(|info| info.op == Op::SetPaletteFade)
            .unwrap();
        let fade_rev03 = revision_table(V4Revision::Rev03)
            .iter()
            .find(|info| info.op == Op::SetPaletteFade)
            .unwrap();
        assert_eq!(fade_rev00.operands.len(), 7);
        assert_eq!(fade_rev03.operands.len(), 6);
        let text_rev00 = revision_table(V4Revision::Rev00)
            .iter()
            .find(|info| info.op == Op::LoadTextFile)
            .unwrap();
        let text_rev03 = revision_table(V4Revision::Rev03)
            .iter()
            .find(|info| info.op == Op::LoadTextFile)
            .unwrap();
        assert_eq!(text_rev00.operands.len(), 3);
        assert_eq!(text_rev03.operands.len(), 2);
        assert_eq!(opcode_by_name(v4, V4Revision::Rev03, "not-an-op"), None);
    }

    #[test]
    fn writeback_table_covers_control_flow_and_engine_ops() {
        assert!(!op_writes_back(Op::IfEqual));
        assert!(!op_writes_back(Op::Else));
        assert!(!op_writes_back(Op::CallFunction));
        assert!(!op_writes_back(Op::Return));
        assert!(!op_writes_back(Op::DrawSprite));
        assert!(!op_writes_back(Op::LoadTextFile));
        // foreach writes the entity reference back on the success path; the VM suppresses the
        // write-back dynamically on the exit paths, like upstream's `opcodeSize = 0`.
        assert!(op_writes_back(Op::ForEachActive));
        assert!(op_writes_back(Op::ForEachAll));
        assert!(op_writes_back(Op::Equal));
        assert!(op_writes_back(Op::Add));
        assert!(op_writes_back(Op::GetTableValue));
        assert!(op_writes_back(Op::Interpolate));
        assert!(op_writes_back(Op::Get16x16TileInfo));
        assert!(op_writes_back(Op::CallNativeFunction2));
    }
}
