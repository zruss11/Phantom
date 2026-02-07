//! Minimal embedding generation for the default local model.
//!
//! This is intentionally dependency-light (no `tokenizers` crate) to keep builds
//! stable in offline-ish environments. For the default model we ship a classic
//! BERT WordPiece `vocab.txt`, so we implement a small tokenizer here.

use crate::embedding_model;
use ort::{session::Session, value::Tensor};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::State;

const DEFAULT_MAX_SEQ_LEN: usize = 128;

#[derive(Debug, Clone)]
struct BertWordPieceTokenizer {
    vocab: HashMap<String, i64>,
    unk_id: i64,
    pad_id: i64,
    cls_id: i64,
    sep_id: i64,
    do_lower_case: bool,
}

impl BertWordPieceTokenizer {
    fn from_vocab_file(path: &PathBuf) -> Result<Self, String> {
        let contents =
            fs::read_to_string(path).map_err(|e| format!("Failed to read vocab.txt: {e}"))?;
        let mut vocab: HashMap<String, i64> = HashMap::new();
        for (i, line) in contents.lines().enumerate() {
            let tok = line.trim();
            if tok.is_empty() {
                continue;
            }
            vocab.insert(tok.to_string(), i as i64);
        }

        let get = |t: &str| {
            vocab
                .get(t)
                .copied()
                .ok_or_else(|| format!("vocab.txt missing required token: {t}"))
        };

        Ok(Self {
            unk_id: get("[UNK]")?,
            pad_id: get("[PAD]")?,
            cls_id: get("[CLS]")?,
            sep_id: get("[SEP]")?,
            vocab,
            do_lower_case: true,
        })
    }

    fn tokenize_basic(&self, text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();

        let push_cur = |cur: &mut String, out: &mut Vec<String>| {
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
        };

        for ch in text.chars() {
            if ch.is_whitespace() {
                push_cur(&mut cur, &mut out);
                continue;
            }
            if ch.is_ascii_punctuation() {
                push_cur(&mut cur, &mut out);
                out.push(ch.to_string());
                continue;
            }
            cur.push(ch);
        }
        push_cur(&mut cur, &mut out);

        if self.do_lower_case {
            out.into_iter().map(|t| t.to_lowercase()).collect()
        } else {
            out
        }
    }

    fn wordpiece(&self, token: &str) -> Vec<i64> {
        if token.is_empty() {
            return Vec::new();
        }
        if let Some(id) = self.vocab.get(token) {
            return vec![*id];
        }

        let chars: Vec<char> = token.chars().collect();
        let mut start = 0usize;
        let mut sub_tokens: Vec<i64> = Vec::new();

        while start < chars.len() {
            let mut end = chars.len();
            let mut found: Option<(i64, usize)> = None;

            while end > start {
                let slice: String = chars[start..end].iter().collect();
                let candidate = if start == 0 {
                    slice
                } else {
                    format!("##{}", slice)
                };

                if let Some(id) = self.vocab.get(&candidate) {
                    found = Some((*id, end));
                    break;
                }
                end -= 1;
            }

            let Some((id, next)) = found else {
                return vec![self.unk_id];
            };
            sub_tokens.push(id);
            start = next;
        }

        sub_tokens
    }

    fn encode(&self, text: &str, max_len: usize) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
        let mut ids: Vec<i64> = Vec::with_capacity(max_len);
        let mut mask: Vec<i64> = Vec::with_capacity(max_len);
        let mut type_ids: Vec<i64> = Vec::with_capacity(max_len);

        ids.push(self.cls_id);

        for tok in self.tokenize_basic(text) {
            for id in self.wordpiece(&tok) {
                if ids.len() + 1 >= max_len {
                    break;
                }
                ids.push(id);
            }
            if ids.len() + 1 >= max_len {
                break;
            }
        }

        ids.push(self.sep_id);

        let real_len = ids.len().min(max_len);
        ids.truncate(max_len);

        for _ in 0..real_len {
            mask.push(1);
            type_ids.push(0);
        }
        while ids.len() < max_len {
            ids.push(self.pad_id);
            mask.push(0);
            type_ids.push(0);
        }

        (ids, mask, type_ids)
    }
}

#[derive(Debug)]
struct EmbeddingRuntime {
    model_id: String,
    dims: usize,
    max_seq_len: usize,
    session: Session,
    tokenizer: BertWordPieceTokenizer,
    input_ids_name: String,
    attention_mask_name: String,
    token_type_ids_name: Option<String>,
}

impl EmbeddingRuntime {
    fn load(model_id: &str, max_seq_len: usize) -> Result<Self, String> {
        let spec = embedding_model::default_model_spec();
        let dims = spec.dims as usize;

        let dir = embedding_model::model_dir(model_id);
        let onnx_path = dir.join("onnx").join("model.onnx");
        let vocab_path = dir.join("vocab.txt");

        if !onnx_path.exists() {
            return Err("Embedding model is not downloaded (missing onnx/model.onnx)".to_string());
        }
        if !vocab_path.exists() {
            return Err("Embedding model is not downloaded (missing vocab.txt)".to_string());
        }

        // Best-effort: let `ort` initialize a default environment if not yet configured.
        let session = Session::builder()
            .map_err(|e| format!("Failed to create ORT session builder: {e}"))?
            .commit_from_file(&onnx_path)
            .map_err(|e| format!("Failed to load embedding model: {e}"))?;

        let tokenizer = BertWordPieceTokenizer::from_vocab_file(&vocab_path)?;

        // Resolve input names (some exports vary slightly).
        let mut input_ids_name: Option<String> = None;
        let mut attention_mask_name: Option<String> = None;
        let mut token_type_ids_name: Option<String> = None;
        for inp in &session.inputs {
            let name = inp.name.as_str();
            if input_ids_name.is_none() && name.contains("input_ids") {
                input_ids_name = Some(inp.name.clone());
            } else if attention_mask_name.is_none() && name.contains("attention_mask") {
                attention_mask_name = Some(inp.name.clone());
            } else if token_type_ids_name.is_none() && name.contains("token_type_ids") {
                token_type_ids_name = Some(inp.name.clone());
            }
        }

        let input_ids_name =
            input_ids_name.ok_or_else(|| "Model missing input_ids input".to_string())?;
        let attention_mask_name =
            attention_mask_name.ok_or_else(|| "Model missing attention_mask input".to_string())?;

        Ok(Self {
            model_id: model_id.to_string(),
            dims,
            max_seq_len,
            session,
            tokenizer,
            input_ids_name,
            attention_mask_name,
            token_type_ids_name,
        })
    }

    fn embed(&mut self, text: &str) -> Result<Vec<f32>, String> {
        let (ids, mask, type_ids) = self.tokenizer.encode(text, self.max_seq_len);

        let ids_tensor = Tensor::<i64>::from_array(([1usize, self.max_seq_len], ids))
            .map_err(|e| format!("Failed to create input_ids tensor: {e}"))?;
        let mask_tensor = Tensor::<i64>::from_array(([1usize, self.max_seq_len], mask.clone()))
            .map_err(|e| format!("Failed to create attention_mask tensor: {e}"))?;

        let mut inputs = ort::inputs! {
            self.input_ids_name.as_str() => ids_tensor,
            self.attention_mask_name.as_str() => mask_tensor,
        };

        if let Some(name) = self.token_type_ids_name.as_ref() {
            let type_tensor = Tensor::<i64>::from_array(([1usize, self.max_seq_len], type_ids))
                .map_err(|e| format!("Failed to create token_type_ids tensor: {e}"))?;
            inputs.push((name.as_str().into(), type_tensor.into()));
        }

        let outputs = self
            .session
            .run(inputs)
            .map_err(|e| format!("Failed to run embedding model: {e}"))?;

        // Prefer any [1, dims] output; otherwise fall back to mean pooling over a [1, seq, hidden] output.
        let mut pooled: Option<Vec<f32>> = None;
        for k in outputs.keys() {
            let v = &outputs[k];
            if let Ok((shape, data)) = v.try_extract_tensor::<f32>() {
                if shape.len() == 2
                    && shape[0] == 1
                    && shape[1] as usize == self.dims
                    && data.len() == self.dims
                {
                    pooled = Some(data.to_vec());
                    break;
                }
            }
        }

        let mut emb = if let Some(v) = pooled {
            v
        } else {
            let v = &outputs[0];
            let (shape, data) = v
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("Unexpected model output type: {e}"))?;

            if shape.len() != 3 || shape[0] != 1 {
                return Err(format!(
                    "Unexpected embedding output shape: {:?}",
                    shape.as_ref()
                ));
            }
            let seq_len = shape[1] as usize;
            let hidden = shape[2] as usize;
            if hidden != self.dims {
                return Err(format!(
                    "Unexpected embedding dims: got {hidden}, expected {}",
                    self.dims
                ));
            }

            let mut sum = vec![0f32; hidden];
            let mut count = 0f32;
            for i in 0..seq_len.min(self.max_seq_len) {
                if mask.get(i).copied().unwrap_or(0) == 0 {
                    continue;
                }
                count += 1.0;
                let off = i * hidden;
                for j in 0..hidden {
                    sum[j] += data[off + j];
                }
            }
            let denom = count.max(1.0);
            for x in &mut sum {
                *x /= denom;
            }
            sum
        };

        // L2 normalize (standard for cosine similarity).
        let norm = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut emb {
                *x /= norm;
            }
        }

        Ok(emb)
    }
}

static RUNTIME: OnceLock<Mutex<Option<EmbeddingRuntime>>> = OnceLock::new();

fn with_runtime<T>(
    model_id: &str,
    max_seq_len: usize,
    f: impl FnOnce(&mut EmbeddingRuntime) -> T,
) -> Result<T, String> {
    let m = RUNTIME.get_or_init(|| Mutex::new(None));
    let mut guard = m
        .lock()
        .map_err(|_| "Embedding runtime mutex poisoned".to_string())?;

    let reload = guard
        .as_ref()
        .map(|rt| rt.model_id != model_id || rt.max_seq_len != max_seq_len)
        .unwrap_or(true);
    if reload {
        *guard = Some(EmbeddingRuntime::load(model_id, max_seq_len)?);
    }

    Ok(f(guard.as_mut().expect("just loaded")))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingGenerateRequest {
    pub text: String,
    pub model_id: Option<String>,
    pub max_seq_len: Option<u32>,
}

#[tauri::command]
pub async fn embedding_generate(
    state: State<'_, crate::AppState>,
    req: EmbeddingGenerateRequest,
) -> Result<Vec<f32>, String> {
    let model_id = req
        .model_id
        .unwrap_or_else(|| embedding_model::DEFAULT_EMBEDDING_MODEL_ID.to_string());
    let max_seq_len = req
        .max_seq_len
        .map(|n| n.clamp(8, 512) as usize)
        .unwrap_or(DEFAULT_MAX_SEQ_LEN);

    // If a download is in-flight, don't contend with it; the UI should wait for Ready.
    {
        let mgr = state.embedding_models.lock().await;
        if mgr.status.state == embedding_model::EmbeddingModelState::Downloading {
            return Err("Embedding model is downloading".to_string());
        }
    }

    let text = req.text;
    tokio::task::spawn_blocking(move || with_runtime(&model_id, max_seq_len, |rt| rt.embed(&text))?)
        .await
        .map_err(|e| format!("Embedding worker failed: {e}"))?
}
