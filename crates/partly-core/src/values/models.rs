// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TypeDefinition {
    #[cfg_attr(feature = "openapi", schema(max_properties = 2))]
    Enum {
        #[cfg_attr(feature = "openapi", schema(min_items = 1, max_items = 64))]
        values: Vec<String>,
    },
    #[cfg_attr(feature = "openapi", schema(max_properties = 2))]
    Record {
        #[cfg_attr(feature = "openapi", schema(schema_with = crate::openapi::record_fields))]
        fields: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    pub value: Value,
    pub kind: String,
    pub variant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub named_type: Option<String>,
}
