//! Drawing: the software canvas, text rasterised into it, figures too small
//! for text set into it pixel by pixel, and a skin -- the compiled layout of
//! a widget -- drawn and hit-tested on the two, in the colour the user picked
//! (`tint`).
//!
//! A skin asks whatever it is drawing for the values that change through
//! `skin::Hooks`, so nothing here knows which widget that is.

pub mod canvas;
pub mod figures;
pub mod skin;
pub mod text;
pub mod tint;
