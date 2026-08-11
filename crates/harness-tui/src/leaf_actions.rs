//! Deterministic leaf action helper types for TUI shards.
//!
//! These types are plain value objects with no shared registry or app-state
//! dependency.
//!
//! Integration owner: Todo 28. Leaf modules are contributed by Todos 23-26
//! and must not be edited by the integrator.

pub mod action;
pub mod group_b_composer_modes;
pub mod group_c_screen_modes;
pub mod group_d_dashboard;
pub mod group_e_media;
pub mod group_f_notices;
pub mod group_g_extensions;
pub mod group_h_navigation;
pub mod group_i_preferences;
pub mod overlay_session_model;

pub use action::ActionLeaf;
