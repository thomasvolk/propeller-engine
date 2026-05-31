// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

pub mod handler;
pub mod server;
pub mod types;

pub use server::run_ipc_server;
pub use types::{EngineSettings, EngineMode};
