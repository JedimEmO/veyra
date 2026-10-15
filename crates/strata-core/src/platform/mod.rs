//! What differs between a native process and a browser, behind one name each:
//! a parallel map that runs on threads or in turn, and the clock. No Bevy.
//!
//! Systems and generators above call these and never ask which target they
//! run on (ticket 0013).

pub mod par;
pub mod time;
