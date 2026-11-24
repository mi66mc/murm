//! Public-state terminal UI. Commands and widgets call the same ClientService.
mod actions;
mod editor;
mod render;
mod runtime;
mod state;
mod tasks;
pub mod terminal;
mod text;
mod views;
pub use actions::*;
pub use editor::*;
pub use render::render;
pub use runtime::{UiEffect, handle_key, handle_paste, run};
pub use state::*;
pub use tasks::*;
pub use text::sanitize;
pub use views::outbox::destination_label;
pub use views::profiles::{Form, FormKind, FormOutcome, FormRequest, execute_form};
