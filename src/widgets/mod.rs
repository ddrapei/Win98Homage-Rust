//! The widgets, and what it is to be one.
//!
//! `widget` is the interface: the `Widget` trait, and the few types that
//! cross it.  The host knows a widget only through that, so each of the
//! others is one widget, whole, in one file -- and another widget is another
//! file here and a line in the host's list, with none of these changed.

pub mod widget;

pub mod datetime;
pub mod notes;
pub mod rain;
pub mod resources;
pub mod weather;
