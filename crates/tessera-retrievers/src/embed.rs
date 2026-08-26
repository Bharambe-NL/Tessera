//! Embeddings, behind a trait.
//!
//! Doc 05 section 8.2 wants hybrid retrieval: "BM25 plus vector, fused by
//! reciprocal rank". Doc 10 section 3 wants the model local by default so that
//! chunk text does not leave the machine unless a user chooses it, and doc 10
//! section 17 question 2 leaves the model choice to be settled on the synthetic
//! recall numbers.
//!
//! The trait is what makes that question answerable later rather than now. A
//! model swap is one implementation, and the deterministic stand in below lets
//! every test that is not about embedding quality run without downloading half
//! a gigabyte or touching the network.
//!
//! The default model is multilingual on purpose. The corpus carries Dutch
//! documents, and a user's own folder is under no obligation to be in English.
//! An English only model does not fail on Dutch text; it embeds it into a
//! region of the space that means nothing, which is worse, because the failure
//! is invisible in every aggregate number.

use std::sync::Mutex;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("the embedding model could not be prepared: {0}")]
    Unavailable(String),
    #[error("embedding failed: {0}")]
    Failed(String),
}

/// Turns text into vectors. One call per batch, because the model amortises.
pub trait Embedder: Send + Sync {
    /// Vectors in the same order as the input.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError>;

    /// The width of every vector this embedder returns.
    fn dimensions(&self) -> usize;

    /// What went into the index, so a later change of model is detectable
    /// rather than silently mixing two vector spaces in one table.
    fn model_id(&self) -> &str;
}

/// Cosine similarity. Both sides are assumed non empty and the same width.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a <= f32::EPSILON || norm_b <= f32::EPSILON {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Pack a vector for the blob column, little endian.
pub fn to_blob(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// Unpack a vector from the blob column. A blob of the wrong length is treated
/// as absent rather than as a vector of nonsense.
pub fn from_blob(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.is_empty() || !blob.len().is_multiple_of(4) {
        return None;
    }
    Some(
        blob.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
    )
}

// ------------------------------------------------------------------ local --

/// The default: a local model, run in process.
///
/// Candle rather than an ONNX runtime, for two reasons that both showed up the
/// moment it was tried. Doc 10 section 3 names candle. And the prebuilt ONNX
/// Runtime binary needs a newer MSVC standard library than this toolchain
/// provides, so every executable linking it failed with unresolved STL symbols
/// (BN-040). Candle is pure Rust: it compiles with whatever compiles the rest
/// of the workspace, and it leaves nothing native to sign or ship at M13.
pub struct LocalEmbedder {
    model: Mutex<candle_transformers::models::bert::BertModel>,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
    model_id: String,
    dimensions: usize,
}

impl LocalEmbedder {
    /// Doc 10 section 3: local by default, so chunk text stays on the machine.
    ///
    /// Weights are downloaded once and cached by `hf-hub`. That download is the
    /// cost doc 10 section 17 question 2 asks whether to pay, and the answer is
    /// the recall number this milestone measures.
    pub fn multilingual() -> Result<Self, EmbedError> {
        Self::from_hub("intfloat/multilingual-e5-small", 384)
    }

    pub fn from_hub(repo: &str, dimensions: usize) -> Result<Self, EmbedError> {
        use candle_core::{DType, Device};
        use candle_nn::VarBuilder;
        use candle_transformers::models::bert::{BertModel, Config};
        use hf_hub::api::sync::Api;

        let api = Api::new().map_err(|e| EmbedError::Unavailable(e.to_string()))?;
        let repo_handle = api.model(repo.to_string());

        let config_path = repo_handle
            .get("config.json")
            .map_err(|e| EmbedError::Unavailable(format!("config.json: {e}")))?;
        let tokenizer_path = repo_handle
            .get("tokenizer.json")
            .map_err(|e| EmbedError::Unavailable(format!("tokenizer.json: {e}")))?;
        let weights_path = repo_handle
            .get("model.safetensors")
            .map_err(|e| EmbedError::Unavailable(format!("model.safetensors: {e}")))?;

        let config: Config = serde_json::from_slice(
            &std::fs::read(&config_path).map_err(|e| EmbedError::Unavailable(e.to_string()))?,
        )
        .map_err(|e| EmbedError::Unavailable(format!("config: {e}")))?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| EmbedError::Unavailable(format!("tokenizer: {e}")))?;

        let device = Device::Cpu;
        // Buffered rather than memory mapped. The mmap constructor is `unsafe`
        // because a file changing under the mapping is undefined behaviour, and
        // the workspace forbids unsafe outright. Reading the weights costs one
        // copy at startup and nothing afterwards.
        let weights = std::fs::read(&weights_path)
            .map_err(|e| EmbedError::Unavailable(format!("weights: {e}")))?;
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &device)
            .map_err(|e| EmbedError::Unavailable(format!("weights: {e}")))?;
        let model = BertModel::load(vb, &config)
            .map_err(|e| EmbedError::Unavailable(format!("model: {e}")))?;

        Ok(Self {
            model: Mutex::new(model),
            tokenizer,
            device,
            model_id: repo.to_string(),
            dimensions,
        })
    }
}

impl Embedder for LocalEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        use candle_core::Tensor;

        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // e5 models are trained with this prefix on the indexed side and
        // "query: " on the asking side. Without it the vectors are still
        // vectors and the neighbourhoods are simply wrong.
        let prepared: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();

        let encodings = self
            .tokenizer
            .encode_batch(prepared, true)
            .map_err(|e| EmbedError::Failed(format!("tokenize: {e}")))?;

        let mut out = Vec::with_capacity(encodings.len());
        let model = self
            .model
            .lock()
            .map_err(|_| EmbedError::Failed("the embedder was poisoned by an earlier panic".into()))?;

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let token_ids = Tensor::new(ids, &self.device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| EmbedError::Failed(e.to_string()))?;
            let attention = Tensor::new(mask, &self.device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| EmbedError::Failed(e.to_string()))?;
            let type_ids = token_ids
                .zeros_like()
                .map_err(|e| EmbedError::Failed(e.to_string()))?;

            let hidden = model
                .forward(&token_ids, &type_ids, Some(&attention))
                .map_err(|e| EmbedError::Failed(e.to_string()))?;

            // Mean pooling over the tokens that are not padding. Taking the
            // whole sequence would let padding drag every short passage toward
            // the same point.
            let vector = mean_pool(&hidden, mask).map_err(EmbedError::Failed)?;
            out.push(vector);
        }

        Ok(out)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

/// Average the token vectors, counting only real tokens.
fn mean_pool(hidden: &candle_core::Tensor, mask: &[u32]) -> Result<Vec<f32>, String> {
    let values: Vec<Vec<f32>> = hidden
        .squeeze(0)
        .map_err(|e| e.to_string())?
        .to_vec2()
        .map_err(|e| e.to_string())?;

    let width = values.first().map(Vec::len).unwrap_or(0);
    if width == 0 {
        return Err("the model returned no hidden state".into());
    }

    let mut sum = vec![0.0f32; width];
    let mut counted = 0usize;
    for (row, keep) in values.iter().zip(mask.iter()) {
        if *keep == 0 {
            continue;
        }
        counted += 1;
        for (acc, v) in sum.iter_mut().zip(row.iter()) {
            *acc += v;
        }
    }
    if counted == 0 {
        return Err("every token was masked out".into());
    }
    for v in &mut sum {
        *v /= counted as f32;
    }
    Ok(sum)
}

// ------------------------------------------------------------------- mock --

/// A deterministic stand in, for tests that are not about embedding quality.
///
/// Hashed character trigrams into a fixed width. It is not semantic and does
/// not pretend to be: two texts sharing wording land near each other, and
/// nothing else does. That is enough to prove fusion, persistence and ordering
/// work, and it keeps the test suite free of a model download.
pub struct HashEmbedder {
    dimensions: usize,
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self { dimensions: 64 }
    }
}

impl HashEmbedder {
    pub fn with_dimensions(dimensions: usize) -> Self {
        Self { dimensions: dimensions.max(1) }
    }

    fn vector(&self, text: &str) -> Vec<f32> {
        let mut out = vec![0.0f32; self.dimensions];
        let lower = text.to_lowercase();
        let chars: Vec<char> = lower.chars().filter(|c| !c.is_whitespace()).collect();
        for window in chars.windows(3) {
            let mut hash: u64 = 1469598103934665603;
            for c in window {
                hash ^= *c as u64;
                hash = hash.wrapping_mul(1099511628211);
            }
            let slot = (hash % self.dimensions as u64) as usize;
            out[slot] += 1.0;
        }
        out
    }
}

impl Embedder for HashEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|t| self.vector(t)).collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        "hash-trigram-test"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_is_one_for_a_vector_against_itself() {
        let v = vec![0.3, 0.4, 0.5];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn cosine_is_zero_for_orthogonal_and_for_mismatched_widths() {
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    #[test]
    fn a_zero_vector_scores_zero_rather_than_dividing_by_zero() {
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn a_vector_survives_the_blob_round_trip() {
        let v = vec![0.5f32, -0.25, 1.75, 0.0];
        let back = from_blob(&to_blob(&v)).expect("round trip");
        assert_eq!(v, back);
    }

    #[test]
    fn a_blob_of_the_wrong_length_is_absent_rather_than_nonsense() {
        assert!(from_blob(&[1, 2, 3]).is_none());
        assert!(from_blob(&[]).is_none());
    }

    #[test]
    fn the_test_embedder_puts_similar_wording_closer_than_unrelated_wording() {
        let e = HashEmbedder::default();
        let v = e
            .embed(&[
                "the minimum own funds requirement".to_string(),
                "the minimum own funds requirement is 8.4 percent".to_string(),
                "why do cats purr".to_string(),
            ])
            .expect("embeds");
        let near = cosine(&v[0], &v[1]);
        let far = cosine(&v[0], &v[2]);
        assert!(near > far, "near {near} was not closer than far {far}");
    }

    #[test]
    fn the_test_embedder_is_deterministic() {
        let e = HashEmbedder::default();
        let a = e.embed(&["a sentence".to_string()]).expect("a");
        let b = e.embed(&["a sentence".to_string()]).expect("b");
        assert_eq!(a, b);
    }

    #[test]
    fn embedding_nothing_returns_nothing() {
        assert!(HashEmbedder::default().embed(&[]).expect("empty").is_empty());
    }
}
