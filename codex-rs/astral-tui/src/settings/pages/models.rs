//! Models & Providers Settings subpage.
//!
//! The existing provider/model manager remains the implementation owner while
//! `/settings` owns its lifecycle. Keeping this boundary explicit avoids
//! duplicating provider discovery, capability editing, or config-write logic.

pub(in crate::settings) use crate::models_manager::ModelsManagerInput;
pub(in crate::settings) use crate::models_manager::ModelsManagerState;
pub(in crate::settings) use crate::models_manager::handle_key;
pub(in crate::settings) use crate::models_manager::handle_mouse;
pub(in crate::settings) use crate::models_manager::handle_paste;
pub(in crate::settings) use crate::models_manager::render;
