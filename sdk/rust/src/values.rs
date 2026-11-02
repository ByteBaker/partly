// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Client, Context, Error};
use serde_json::{Map, Value};

impl Client {
    pub fn evaluate_string(&self, key: &str, context: &Context) -> Result<String, Error> {
        serde_json::from_value(self.resolve(key, context, "string", None)?.value)
            .map_err(|_| Error::InvalidPolicy("Invalid result value".into()))
    }

    pub fn evaluate_integer(&self, key: &str, context: &Context) -> Result<i64, Error> {
        serde_json::from_value(self.resolve(key, context, "integer", None)?.value)
            .map_err(|_| Error::InvalidPolicy("Invalid result value".into()))
    }

    pub fn evaluate_float(&self, key: &str, context: &Context) -> Result<f64, Error> {
        serde_json::from_value(self.resolve(key, context, "float", None)?.value)
            .map_err(|_| Error::InvalidPolicy("Invalid result value".into()))
    }

    pub fn evaluate_object(
        &self,
        key: &str,
        context: &Context,
    ) -> Result<Map<String, Value>, Error> {
        serde_json::from_value(self.resolve(key, context, "object", None)?.value)
            .map_err(|_| Error::InvalidPolicy("Invalid result value".into()))
    }

    /// Partly enhancement: require the exact dashboard-defined type name.
    pub fn evaluate_named(
        &self,
        key: &str,
        context: &Context,
        expected_type: &str,
    ) -> Result<Value, Error> {
        Ok(self
            .resolve(key, context, "named", Some(expected_type))?
            .value)
    }
}

#[cfg(test)]
mod tests;
