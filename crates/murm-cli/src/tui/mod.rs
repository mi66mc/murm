//! Public-state terminal UI. Commands and widgets call the same ClientService.
mod state;
mod actions;
pub mod terminal;
mod tasks;
mod render;
mod views;
pub use state::*;
pub use actions::*;
pub use tasks::*;
pub use render::{render,sanitize};
pub use views::profiles::{Form,FormKind,FormRequest,FormOutcome,execute_form};
