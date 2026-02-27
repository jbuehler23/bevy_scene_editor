use bevy::color::palettes::tailwind;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Corner radius
// ---------------------------------------------------------------------------

pub const CORNER_RADIUS: Val = Val::Px(2.0);
pub const CORNER_RADIUS_LG: Val = Val::Px(4.0);

// ---------------------------------------------------------------------------
// Primary / accent colors
// ---------------------------------------------------------------------------

pub const PRIMARY_COLOR: Srgba = tailwind::BLUE_500;

// ---------------------------------------------------------------------------
// Background colors (Tailwind Zinc dark palette)
// ---------------------------------------------------------------------------

/// Root window background
pub const WINDOW_BG: Color = Color::Srgba(tailwind::ZINC_900);
/// Panel body background
pub const PANEL_BG: Color = Color::Srgba(tailwind::ZINC_800);
/// Panel header bar background
pub const PANEL_HEADER_BG: Color = Color::Srgba(tailwind::ZINC_700);
/// Toolbar background
pub const TOOLBAR_BG: Color = Color::Srgba(tailwind::ZINC_800);
/// Text input background
pub const INPUT_BG: Color = Color::Srgba(tailwind::ZINC_900);
/// Context menu / dropdown background
pub const MENU_BG: Color = Color::Srgba(Srgba { red: tailwind::ZINC_800.red, green: tailwind::ZINC_800.green, blue: tailwind::ZINC_800.blue, alpha: 0.98 });
/// Status bar background
pub const STATUS_BAR_BG: Color = Color::Srgba(tailwind::ZINC_800);
/// Inactive toolbar button background
pub const TOOLBAR_BUTTON_BG: Color = Color::Srgba(tailwind::ZINC_800);
/// General background color (for widgets)
pub const BACKGROUND_COLOR: Srgba = tailwind::ZINC_800;

// ---------------------------------------------------------------------------
// Borders & separators
// ---------------------------------------------------------------------------

/// Subtle border / separator
pub const BORDER_SUBTLE: Color = Color::Srgba(tailwind::ZINC_700);
/// Strong / emphasized border
pub const BORDER_STRONG: Color = Color::Srgba(tailwind::ZINC_600);
/// Standard border color (for widgets)
pub const BORDER_COLOR: Srgba = tailwind::ZINC_700;

// ---------------------------------------------------------------------------
// Interactive states
// ---------------------------------------------------------------------------

/// Hovered row / item background
pub const HOVER_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.1);
/// Selected item background
pub const SELECTED_BG: Color = Color::srgba(0.0, 0.204, 0.431, 1.0);
/// Selected item border
pub const SELECTED_BORDER: Color = Color::srgba(0.035, 0.290, 0.580, 1.0);
/// Active / pressed background
pub const ACTIVE_BG: Color = Color::Srgba(tailwind::ZINC_600);
/// Drag-drop target highlight
pub const DROP_TARGET_BG: Color = Color::Srgba(Srgba { red: 0.3, green: 0.5, blue: 0.2, alpha: 1.0 });
/// Drag-drop target border accent
pub const DROP_TARGET_BORDER: Color = Color::Srgba(Srgba { red: 0.3, green: 0.7, blue: 0.4, alpha: 1.0 });
/// Root container drag-drop overlay
pub const CONTAINER_DROP_TARGET_BG: Color = Color::Srgba(Srgba { red: 0.2, green: 0.3, blue: 0.2, alpha: 0.3 });
/// Tree connection line color
pub const CONNECTION_LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.2);

// ---------------------------------------------------------------------------
// Entity category colors (for hierarchy tree dots)
// ---------------------------------------------------------------------------

/// Camera entity dot color (blue)
pub const CATEGORY_CAMERA: Color = Color::srgba(0.286, 0.506, 0.710, 1.0);
/// Light entity dot color (yellow)
pub const CATEGORY_LIGHT: Color = Color::srgba(1.0, 0.882, 0.0, 1.0);
/// Mesh entity dot color (orange/brown)
pub const CATEGORY_MESH: Color = Color::srgba(0.710, 0.537, 0.294, 1.0);
/// Scene root dot color (cyan)
pub const CATEGORY_SCENE: Color = Color::srgba(0.0, 0.667, 0.733, 1.0);
/// Generic entity dot color (green)
pub const CATEGORY_ENTITY: Color = Color::srgba(0.259, 0.725, 0.514, 1.0);

// ---------------------------------------------------------------------------
// Text colors
// ---------------------------------------------------------------------------

/// Primary text
pub const TEXT_PRIMARY: Color = Color::Srgba(tailwind::ZINC_200);
/// Secondary / dimmed text
pub const TEXT_SECONDARY: Color = Color::Srgba(tailwind::ZINC_400);
/// Accent / link text
pub const TEXT_ACCENT: Color = Color::Srgba(tailwind::BLUE_400);
/// Accent hover — lighter blue
pub const TEXT_ACCENT_HOVER: Color = Color::Srgba(tailwind::BLUE_300);
/// Body text color (widget standard)
pub const TEXT_BODY_COLOR: Srgba = tailwind::ZINC_200;
/// Display text color (bright)
pub const TEXT_DISPLAY_COLOR: Srgba = tailwind::ZINC_50;
/// Muted text color
pub const TEXT_MUTED_COLOR: Srgba = tailwind::ZINC_400;

// ---------------------------------------------------------------------------
// Inspector type-indicator label colors (muted tints for field labels)
// ---------------------------------------------------------------------------

/// Numeric (f32/f64/int) field label — green tint
pub const TYPE_NUMERIC: Color = Color::srgb(0.55, 0.78, 0.55);
/// Boolean field label — blue tint
pub const TYPE_BOOL: Color = Color::srgb(0.55, 0.65, 0.85);
/// String field label — orange tint
pub const TYPE_STRING: Color = Color::srgb(0.85, 0.70, 0.45);
/// Entity reference field label — white
pub const TYPE_ENTITY: Color = Color::Srgba(tailwind::ZINC_300);
/// Enum field label — purple tint
pub const TYPE_ENUM: Color = Color::srgb(0.72, 0.55, 0.82);

// ---------------------------------------------------------------------------
// File browser icon colors
// ---------------------------------------------------------------------------

/// Directory icon — warm yellow
pub const DIR_ICON_COLOR: Color = Color::srgb(0.9, 0.8, 0.3);
/// Generic file icon — grey
pub const FILE_ICON_COLOR: Color = Color::Srgba(tailwind::ZINC_400);

// ---------------------------------------------------------------------------
// Text sizes
// ---------------------------------------------------------------------------

pub const TEXT_SIZE_SM: f32 = 10.0;
pub const TEXT_SIZE: f32 = 12.0;
pub const TEXT_SIZE_LG: f32 = 14.0;
pub const TEXT_SIZE_XL: f32 = 18.0;

// Keep old names as aliases for existing code
pub const FONT_SM: f32 = TEXT_SIZE_SM;
pub const FONT_MD: f32 = TEXT_SIZE;
pub const FONT_LG: f32 = TEXT_SIZE_LG;
pub const ICON_LG: f32 = 24.0;

// ---------------------------------------------------------------------------
// Spacing & sizing constants
// ---------------------------------------------------------------------------

pub const SPACING_XS: f32 = 2.0;
pub const SPACING_SM: f32 = 4.0;
pub const SPACING_MD: f32 = 8.0;
pub const SPACING_LG: f32 = 12.0;

pub const ROW_HEIGHT: f32 = 24.0;
pub const HEADER_HEIGHT: f32 = 28.0;
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
pub const MENU_BAR_HEIGHT: f32 = 28.0;
pub const INPUT_HEIGHT: f32 = 28.0;

pub const BORDER_RADIUS_SM: f32 = 3.0;
pub const BORDER_RADIUS_MD: f32 = 4.0;
pub const BORDER_RADIUS_LG: f32 = 5.0;
