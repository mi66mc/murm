//! Public-state terminal UI. Commands and widgets call the same ClientService.
mod state;
mod actions;
pub mod terminal;
pub use state::*;
pub use actions::*;
