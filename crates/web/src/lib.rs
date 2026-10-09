//! Open Model Catalog website (Topcoat, server-rendered, no client script)
//! and the static JSON API.

pub mod app;
pub mod components;
mod fonts;
pub mod format;
pub mod listing;
mod routes;
pub mod security;
pub mod state;
mod ui;
pub mod views;

pub use app::router;
pub use state::AppState;
/// Re-exported so the CLI serves with the same Topcoat version.
pub use topcoat;
pub use topcoat::asset::{AssetBundle, AssetConfig};
