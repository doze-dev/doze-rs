//! The component library: maud functions that return markup, one file per family. A page uses components and the
//! kit's classes through them, never raw class strings for a variant: a misspelt variant is an error, not a bad page.

pub mod button;
pub mod client;
pub mod code;
pub mod components;
pub mod dialog;
pub mod disclosure;
pub mod filter;
pub mod form;
pub mod glyph;
pub mod glyph_extra;
pub mod glyph_groups;
pub mod glyph_live;
pub mod glyph_log;
pub mod glyph_settings;
pub mod inplace;
pub mod kafka;
pub mod layout;
pub mod notice;
pub mod overlay;
pub mod shell;
pub mod table;
pub mod write;
