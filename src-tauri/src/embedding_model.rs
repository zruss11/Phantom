//! Local embedding model metadata + filesystem layout.
//!
//! The actual download/inference pipeline is implemented separately; this module
//! defines stable IDs and where assets live on disk.

#![allow(dead_code)]

use crate::local_asr_model;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_EMBEDDING_MODEL_ID: &str = "all-minilm-l6-v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelSpec {
    pub id: String,
    pub label: String,
    pub dims: u32,
    pub approx_size_mb: u32,
}

pub fn embeddings_root_dir() -> PathBuf {
    local_asr_model::models_root_dir().join("embeddings")
}

pub fn model_dir(model_id: &str) -> PathBuf {
    embeddings_root_dir().join(model_id)
}

pub fn default_model_spec() -> EmbeddingModelSpec {
    // Widely used small SentenceTransformers model (384 dims).
    EmbeddingModelSpec {
        id: DEFAULT_EMBEDDING_MODEL_ID.to_string(),
        label: "all-MiniLM-L6-v2".to_string(),
        dims: 384,
        approx_size_mb: 90,
    }
}
