#[allow(unused_imports)]
pub mod common;
pub mod ai;
pub mod b24;
pub mod ds;
pub mod calendar;
pub mod mail;
pub mod mattermost;
pub mod telegram;
pub mod weather;

pub use ai::*;
pub use b24::*;
pub use ds::*;
pub use calendar::*;
pub use mail::*;
pub use mattermost::*;
pub use telegram::*;
pub use weather::*;
