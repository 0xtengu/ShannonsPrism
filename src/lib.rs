// Copyright (C) 2026 0xtengu
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

#![allow(non_snake_case)]
//! Library surface so the integration tests can reach the internals; `main.rs` is the interface.

pub mod carve;
pub mod codecs;
pub mod corpus;
pub mod emap;
pub mod format;
pub mod libs;
pub mod pack;
pub mod registry;
pub mod score;
