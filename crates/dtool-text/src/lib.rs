pub mod metrics;
pub mod table_layout;
pub mod fonts;
pub mod shape;
pub mod story;
pub mod char_style;
pub mod para_style;
pub mod layout;
pub mod linebreak;
pub mod path_text;
pub mod frame;
pub mod filler;
pub mod import_text;
pub mod bridge;
pub mod hyphen;
pub mod special;
pub mod notes_layout;
pub mod canvas_text;
pub mod glyphs;
pub mod autocorrect;
pub mod decorations;
pub mod font_preview;
pub mod text_find;
pub mod spelling;
pub mod spell_live;
pub mod spelling_panel;
pub mod text_host;
pub mod text_faces;

pub use table_layout::{layout_table, TableLayout, CellLayout};
pub use fonts::{FontFace, FontId, FontLibrary};
pub use shape::{ShapedGlyph, ShapedRun, shape_run, shape_run_with, shape_styled, GlyphTransform, ShapeOptions, parse_language, SYNTHETIC_ITALIC_DEG};
pub use story::{StoryId, TextRange, CharStyle, ParaStyle, Align, Run, Story, CharPatch, CharQuery, CharPatchField, Line};
pub use layout::layout_story;
pub use path_text::{PathFlow, PathTextData};
pub use frame::{layout_chain, caret_at_point, ChainLayout, FrameChain, FrameProps, VAlign};
pub use bridge::{resolved_frame_props, story_from_core};
pub use hyphen::Hyphenator;
pub use special::{BreakKind, Special};
pub use para_style::Hyphenation;
pub use notes_layout::{layout_notes, NotePlacement};

pub fn crate_ready() -> bool {
    true
}

pub use char_style::{Caps, CaseOp, Position, Kerning, Decoration, apply_case};
