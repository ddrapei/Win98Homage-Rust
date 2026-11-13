//! Typing: how the keyboard reaches a widget that never takes it (`keys`),
//! and what the keys then do to a piece of text (`editor`).
//!
//! Neither knows whose text it is: the note and the weather's city field are
//! both typed on with these.

pub mod editor;
pub mod keys;
