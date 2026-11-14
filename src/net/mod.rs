//! The network: one GET (`http`), and the one question that is put to it by
//! more than one widget -- where is this city (`geocode`).
//!
//! None of this ever runs in the program that shows the widgets.  A widget
//! that wants something from the network runs the program again as a child
//! with a job to do; the child loads WinHTTP, asks, writes the answer to a
//! file and exits.  So the DLLs, sockets and TLS state a request drags in
//! live for a second, not for good.

pub mod geocode;
pub mod http;
