//! Drawing: the software canvas, text rasterised into it, and a skin -- the
//! compiled layout of a widget -- drawn and hit-tested on the two.
//!
//! A skin asks whatever it is drawing for the values that change through
//! `skin::Hooks`, so nothing here knows which widget that is.

pub mod canvas;
pub mod skin;
pub mod text;
