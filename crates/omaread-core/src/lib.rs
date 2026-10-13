//! Everything in Omaread that doesn't need a window: theme, styles, and
//! (later) the library, storage and providers. Kept free of GTK so it can be
//! tested on its own.

pub mod color;
pub mod command;
pub mod config;
pub mod db;
pub mod epub;
pub mod library;
pub mod speed;
pub mod style;
pub mod theme;
