//! What is kept on disk between runs, and the files it is kept in:
//! settings.ini, the journal of when the computer was on and unlocked, and
//! the photos the frames have read down already.

pub mod cache;
pub mod settings;
pub mod usage;
