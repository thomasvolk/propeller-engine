// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use std::path::PathBuf;

pub fn resolve() -> PathBuf {
    propeller_common::socket_path::resolve("PROPELLER_CLOCK_SOCK", "/tmp/propeller-clock.sock")
}
