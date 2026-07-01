use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::read_window::{
    normalize_read_limit, normalize_read_offset, READ_DEFAULT_LIMIT, READ_DEFAULT_OFFSET,
};

use super::render::FsReadRenderOptions;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FsReadArgs {
    pub(super) path: String,
    #[serde(default = "default_fs_read_offset")]
    offset: u32,
    #[serde(default = "default_fs_read_limit")]
    limit: u32,
    #[serde(default = "default_fs_read_line_numbers")]
    line_numbers: bool,
    #[serde(default)]
    hashline_anchors: Option<bool>,
}

#[derive(Debug)]
pub(super) struct FsReadRequest {
    pub(super) path: String,
    pub(super) offset: u32,
    pub(super) limit: u32,
    pub(super) render: FsReadRenderOptions,
}

impl FsReadArgs {
    pub(super) fn into_request(self, default_hashline_anchors: bool) -> FsReadRequest {
        FsReadRequest {
            path: self.path,
            offset: normalize_read_offset(self.offset),
            limit: normalize_read_limit(self.limit),
            render: FsReadRenderOptions {
                line_numbers: self.line_numbers,
                hashline_anchors: self.hashline_anchors.unwrap_or(default_hashline_anchors),
            },
        }
    }
}

impl FsReadRequest {
    pub(super) fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    pub(super) fn start_line_index(&self) -> usize {
        (self.offset - 1) as usize
    }

    pub(super) fn line_limit(&self) -> usize {
        self.limit as usize
    }
}

fn default_fs_read_offset() -> u32 {
    READ_DEFAULT_OFFSET
}

fn default_fs_read_limit() -> u32 {
    READ_DEFAULT_LIMIT
}

fn default_fs_read_line_numbers() -> bool {
    true
}

pub(super) fn fs_read_parameters_json_schema(default_hashline_anchors: bool) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string"
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "default": READ_DEFAULT_OFFSET
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "default": READ_DEFAULT_LIMIT
            },
            "line_numbers": {
                "type": "boolean",
                "default": true
            },
            "hashline_anchors": {
                "type": "boolean",
                "default": default_hashline_anchors,
                "description": "When true, render lines as LINE#HASH|text for anchor-driven edit workflows"
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}
