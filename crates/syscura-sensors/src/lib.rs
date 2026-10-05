//! Platform event sources. Each sensor pushes `syscura_core::Event`s into a
//! channel; none of them poll.

pub mod xml;

#[cfg(windows)]
pub mod eventlog;
