//! Windows itself, and nothing that knows what a widget is: the Win32
//! declarations everything else calls through, and the clock.
//!
//! The bottom of the program.  These depend on nothing above them.

pub mod clock;
pub mod sys;
