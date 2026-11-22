//! The widgets, and what it is to be one.
//!
//! `widget` is the interface: the `Widget` trait, and the few types that
//! cross it.  The host knows a widget only through that, so each of the
//! others is one widget, whole, in one file -- and another widget is another
//! file here and a line in the host's list, with none of these changed.
//!
//! `field` and `calendar` are what some of them share: a city -- or any one
//! line -- typed into a field, and a month calendar with its month list.
//! `sky` is the weather's animated look, the picture alone, as `dither` is
//! the Dither Frame's.

pub mod widget;

pub mod calendar;
pub mod field;

pub mod battery;
pub mod claude;
pub mod countdown;
pub mod datetime;
pub mod disk;
pub mod dither;
pub mod flip;
pub mod folders;
pub mod notes;
pub mod photo;
pub mod rain;
pub mod resources;
pub mod sky;
pub mod weather;
pub mod world;
