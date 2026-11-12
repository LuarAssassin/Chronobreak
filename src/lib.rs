//! A retained GUI with explicit UI-thread state bindings.
//!
//! ```no_run
//! use chronobreak::{Application, Element, Result, Signal};
//!
//! fn main() -> Result<()> {
//!     let text = Signal::new(String::from("你好"));
//!     let shown = Signal::new(true);
//!     let prefix = String::from("消息：");
//!     let root = Element::bound_text(&text, move |value| format!("{prefix}{value}"))
//!         .visible(&shown);
//!     Application::new(root).title("Chronobreak").run()
//! }
//! ```

mod application;
mod render;
pub mod signal;
pub mod ui;

pub use application::Application;
pub use render::Result;
pub use signal::Signal;
pub use ui::Element;
