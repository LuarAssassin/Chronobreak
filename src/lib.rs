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
mod children;
mod messages;
mod node;
mod render;
pub mod semantics;
pub mod signal;
pub mod text_edit;
pub mod theme;
pub mod ui;
pub mod visual;
pub use semantics::{ActionKind, ActionResult, Role, SemanticAction, Semantics};
pub use theme::{SemanticTheme, ThemeColor, ThemeController, ThemeMode};

pub use application::Application;
pub use children::{Children, CommandReceipt, TreeError};
pub use messages::{
    MessageCancellation, MessageReceiver, MessageSender, TrySendError, message_channel,
};
pub use node::NodeId;
pub use render::Result;
pub use signal::{Signal, batch};
pub use text_edit::{Direction, EditUnit, HistoryLimits, HistoryStats, Selection, TextEditModel};
pub use ui::{
    Element, EventContext, EventKind, EventPhase, KeyInput, ScrollError, ScrollHandle,
    ScrollReceipt, ScrollSnapshot, UiEvent, WheelInput,
};
pub use visual::{Border, Shadow, linear_to_srgb, srgb_to_linear, srgb8, srgba8};
pub use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase},
    keyboard,
};
