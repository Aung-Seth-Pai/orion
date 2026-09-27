//! Local embedding generation for semantic search.
//!
//! Everything here runs on the user's CPU. The model is `all-MiniLM-L6-v2`
//! (6-layer BERT, 384-dimensional sentence embeddings), executed through
//! `candle` in pure Rust — there is no ONNX Runtime DLL to bundle and no
//! Python in the loop.
//!
//! The weights are the one piece we cannot ship inside the installer: the
//! safetensors file is ~90 MB, so it is fetched once from Hugging Face into
//! the app's local data directory and reused offline forever after. Nothing is sent
//! to Hugging Face except the file requests themselves, and no text the user
//! searches or stores ever leaves the machine.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::Tokenizer;

/// Sentence-embedding width of all-MiniLM-L6-v2. The vector table is declared
/// with this length, so changing the model means a re-index.
pub const EMBEDDING_DIM: usize = 384;

/// MiniLM's trained context window. Longer input is truncated by the tokenizer.
const MAX_SEQUENCE_LEN: usize = 256;

const MODEL_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";
const MODEL_REVISION: &str = "main";
const MODEL_DIR: &str = "models/all-MiniLM-L6-v2";

/// Files pulled on first use, with a floor on the expected size so a truncated
/// or error-page download is rejected rather than cached as a valid model.
const MODEL_FILES: &[(&str, u64)] = &[
    ("config.json", 256),
    ("tokenizer.json", 100_000),
    ("model.safetensors", 50_000_000),
];

/// Loaded once and reused: constructing the model costs far more than running
/// it. Only a *successful* load is stored — caching a failure would make the
/// first call before the download completes poison every later one until the
/// app restarts.
static EMBEDDER: OnceLock<Embedder> = OnceLock::new();

struct Embedder {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

/// Where the weights live, under the caller-supplied root (the *local* app data
/// directory — see `commands::model_root_dir`). Deleting that directory
/// reclaims the full footprint.
pub fn model_dir(root: &Path) -> PathBuf {
    root.join(MODEL_DIR)
}

/// True when every model file is already on disk at a plausible size, meaning
/// [`generate_embedding`] can run without touching the network.
pub fn is_model_downloaded(root: &Path) -> bool {
    let dir = model_dir(root);
    MODEL_FILES.iter().all(|(name, min_bytes)| {
        std::fs::metadata(dir.join(name)).is_ok_and(|m| m.is_file() && m.len() >= *min_bytes)
    })
}

/// Downloads any missing model file into the app data directory.
///
/// Safe to call repeatedly: files already present at a plausible size are
/// skipped, so this is a no-op once the model is in place. Each file is
/// written to a `.part` path and renamed only after a complete transfer, so an
/// interrupted download can never be mistaken for a usable model.
pub async fn ensure_model_downloaded(root: &Path) -> Result<(), String> {
    let dir = model_dir(root);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create model directory {}: {e}", dir.display()))?;

    for (name, min_bytes) in MODEL_FILES {
        let target = dir.join(name);
        if std::fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() >= *min_bytes) {
            continue;
        }

        let url = format!("https://huggingface.co/{MODEL_REPO}/resolve/{MODEL_REVISION}/{name}");
        log::info!("Downloading embedding model file '{name}'");
        download_to_file(&url, &target, *min_bytes).await?;
    }

    log::info!("Embedding model ready at {}", dir.display());
    Ok(())
}

/// How many times a single file is re-attempted before giving up. The weights
/// are ~90 MB, and a dropped connection partway through is common enough that
/// failing the whole setup on the first stall is not acceptable.
const DOWNLOAD_ATTEMPTS: usize = 5;

async fn download_to_file(url: &str, target: &Path, min_bytes: u64) -> Result<(), String> {
    // Appended rather than swapped in via with_extension, which would turn
    // `model.safetensors` into `model.part` and collide across files.
    let partial = {
        let mut name = target.as_os_str().to_owned();
        name.push(".part");
        PathBuf::from(name)
    };

    let mut last_error = String::from("no attempt was made");

    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match download_attempt(url, &partial).await {
            Ok(total) => {
                if total < min_bytes {
                    // A complete response this far below the expected size means
                    // the URL served something other than the model.
                    let _ = tokio::fs::remove_file(&partial).await;
                    return Err(format!(
                        "model download was truncated ({total} bytes, expected at least {min_bytes})"
                    ));
                }
                tokio::fs::rename(&partial, target)
                    .await
                    .map_err(|e| format!("failed to finalise {}: {e}", target.display()))?;
                return Ok(());
            }
            Err(e) => {
                // The partial file is deliberately left in place so the next
                // attempt resumes from where this one stopped.
                log::warn!("Model download attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed: {e}");
                last_error = e;
            }
        }
    }

    Err(format!(
        "model download failed after {DOWNLOAD_ATTEMPTS} attempts: {last_error}"
    ))
}

/// Streams `url` into `partial`, resuming from whatever is already there.
/// Returns the total byte count on a complete transfer.
async fn download_attempt(url: &str, partial: &Path) -> Result<u64, String> {
    use tokio::io::AsyncWriteExt;

    let resume_from = tokio::fs::metadata(partial)
        .await
        .map_or(0, |m| if m.is_file() { m.len() } else { 0 });

    let mut request = reqwest::Client::new().get(url);
    if resume_from > 0 {
        log::info!("Resuming download at {resume_from} bytes");
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("failed to reach the model host: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("model download failed with HTTP {status}"));
    }

    // 206 honours the range; a 200 means the server ignored it and is sending
    // the whole file again, so the partial has to be thrown away.
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT && resume_from > 0;
    let already_on_disk = if resuming { resume_from } else { 0 };
    let expected_total = response.content_length().map(|len| already_on_disk + len);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resuming)
        .truncate(!resuming)
        .open(partial)
        .await
        .map_err(|e| format!("failed to open {}: {e}", partial.display()))?;

    let mut written = already_on_disk;
    let mut response = response;

    // Streamed chunk by chunk so a ~90 MB file never sits in memory whole.
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk)
                    .await
                    .map_err(|e| format!("failed to write model file: {e}"))?;
                written += chunk.len() as u64;
            }
            Ok(None) => break,
            Err(e) => {
                let _ = file.flush().await;
                return Err(format!("model download interrupted: {e}"));
            }
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("failed to flush model file: {e}"))?;

    if let Some(expected) = expected_total {
        if written < expected {
            return Err(format!(
                "connection closed early ({written} of {expected} bytes)"
            ));
        }
    }
    Ok(written)
}

/// Loads the weights and tokenizer from disk. Expensive — call through
/// [`embedder`] so it happens at most once per process.
fn load_embedder(root: &Path) -> Result<Embedder, String> {
    let dir = model_dir(root);
    if !is_model_downloaded(root) {
        return Err(
            "The semantic search model has not been downloaded yet. Run the model setup first."
                .to_string(),
        );
    }

    let config: Config = serde_json::from_slice(
        &std::fs::read(dir.join("config.json"))
            .map_err(|e| format!("failed to read model config: {e}"))?,
    )
    .map_err(|e| format!("failed to parse model config: {e}"))?;

    let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| format!("failed to load tokenizer: {e}"))?;
    // Configured once here rather than per call: cloning a Tokenizer copies the
    // whole vocabulary, which would dominate the cost of indexing many items.
    tokenizer
        .with_truncation(Some(tokenizers::TruncationParams {
            max_length: MAX_SEQUENCE_LEN,
            ..Default::default()
        }))
        .map_err(|e| format!("failed to configure truncation: {e}"))?;

    let device = Device::Cpu;
    // SAFETY: the file is memory-mapped read-only; it is written atomically by
    // `download_to_file` and never mutated afterwards.
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DType::F32, &device)
            .map_err(|e| format!("failed to memory-map model weights: {e}"))?
    };

    let model =
        BertModel::load(vb, &config).map_err(|e| format!("failed to build the model: {e}"))?;

    log::info!("Embedding model loaded on CPU ({EMBEDDING_DIM} dimensions)");
    Ok(Embedder {
        model,
        tokenizer,
        device,
    })
}

fn embedder(root: &Path) -> Result<&'static Embedder, String> {
    if let Some(loaded) = EMBEDDER.get() {
        return Ok(loaded);
    }
    // Two threads racing here both load; the loser's copy is dropped. Loading
    // is rare enough that serialising it is not worth a mutex.
    let loaded = load_embedder(root)?;
    Ok(EMBEDDER.get_or_init(|| loaded))
}

/// Embeds `text` into a unit-length 384-dimensional vector.
///
/// Sentence-transformers models are trained with mean pooling over the final
/// hidden states followed by L2 normalisation; skipping either step produces
/// vectors whose distances are not comparable. Because the result is
/// normalised, cosine similarity reduces to a plain dot product.
///
/// This is CPU-bound and blocking — call it from `spawn_blocking`, never
/// directly on the async runtime.
pub fn generate_embedding(root: &Path, text: &str) -> Result<Vec<f32>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Cannot embed empty text".into());
    }

    let embedder = embedder(root)?;

    let encoding = embedder
        .tokenizer
        .encode(trimmed, true)
        .map_err(|e| format!("failed to tokenize input: {e}"))?;

    let ids = encoding.get_ids();
    if ids.is_empty() {
        return Err("Input produced no tokens".into());
    }
    let mask = encoding.get_attention_mask();

    let device = &embedder.device;
    let token_ids = Tensor::new(ids, device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| format!("failed to build the input tensor: {e}"))?;
    let token_type_ids = token_ids
        .zeros_like()
        .map_err(|e| format!("failed to build the token type tensor: {e}"))?;
    let attention_mask = Tensor::new(mask, device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| format!("failed to build the attention mask: {e}"))?;

    let hidden = embedder
        .model
        .forward(&token_ids, &token_type_ids, Some(&attention_mask))
        .map_err(|e| format!("model inference failed: {e}"))?;

    let pooled = mean_pool(&hidden, mask).map_err(|e| format!("failed to pool the output: {e}"))?;
    let normalised = l2_normalise(&pooled);

    debug_assert_eq!(normalised.len(), EMBEDDING_DIM);
    if normalised.len() != EMBEDDING_DIM {
        return Err(format!(
            "model produced {} dimensions, expected {EMBEDDING_DIM}",
            normalised.len()
        ));
    }
    Ok(normalised)
}

/// Averages the token vectors, counting only real tokens — padding must not
/// drag the sentence vector toward zero.
fn mean_pool(hidden: &Tensor, mask: &[u32]) -> candle_core::Result<Vec<f32>> {
    let tokens: Vec<Vec<f32>> = hidden.squeeze(0)?.to_vec2()?;
    let width = tokens.first().map_or(0, Vec::len);
    let mut summed = vec![0f32; width];
    let mut counted = 0f32;

    for (token, keep) in tokens.iter().zip(mask.iter()) {
        if *keep == 0 {
            continue;
        }
        counted += 1.0;
        for (slot, value) in summed.iter_mut().zip(token.iter()) {
            *slot += value;
        }
    }

    if counted > 0.0 {
        for slot in &mut summed {
            *slot /= counted;
        }
    }
    Ok(summed)
}

/// Scales a vector to unit length, leaving an all-zero vector untouched.
fn l2_normalise(vector: &[f32]) -> Vec<f32> {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return vector.to_vec();
    }
    vector.iter().map(|v| v / norm).collect()
}

/// Packs an embedding into the little-endian f32 blob `sqlite-vec` expects.
pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

/// Reverses [`embedding_to_blob`], rejecting blobs that are not a whole number
/// of f32s of the expected width.
pub fn blob_to_embedding(blob: &[u8]) -> Result<Vec<f32>, String> {
    if blob.len() != EMBEDDING_DIM * 4 {
        return Err(format!(
            "expected a {}-byte embedding blob, got {}",
            EMBEDDING_DIM * 4,
            blob.len()
        ));
    }
    Ok(blob
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_round_trips_an_embedding() {
        let original: Vec<f32> = (0..EMBEDDING_DIM).map(|i| i as f32 / 100.0).collect();
        let blob = embedding_to_blob(&original);
        assert_eq!(blob.len(), EMBEDDING_DIM * 4);
        assert_eq!(blob_to_embedding(&blob).unwrap(), original);
    }

    #[test]
    fn blob_of_the_wrong_width_is_rejected() {
        assert!(blob_to_embedding(&[0u8; 16]).is_err());
        assert!(blob_to_embedding(&[]).is_err());
    }

    #[test]
    fn normalising_produces_unit_length() {
        let normalised = l2_normalise(&[3.0, 4.0]);
        assert!((normalised[0] - 0.6).abs() < 1e-6);
        assert!((normalised[1] - 0.8).abs() < 1e-6);

        let norm = l2_normalise(&[1.0, 2.0, 3.0, 4.0])
            .iter()
            .map(|v| v * v)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalising_leaves_a_zero_vector_alone() {
        assert_eq!(l2_normalise(&[0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn missing_model_is_reported_not_downloaded() {
        let empty = std::env::temp_dir().join("orion-no-such-model-dir");
        assert!(!is_model_downloaded(&empty));
    }

    /// End-to-end check against the real weights. Ignored by default because it
    /// downloads ~90 MB on a cold cache; run it deliberately with
    /// `cargo test -- --ignored --nocapture` after touching anything in the
    /// load or pooling path.
    #[test]
    #[ignore = "downloads the model on first run"]
    fn embeds_text_and_places_related_sentences_closer() {
        let dir = std::env::temp_dir().join("orion-model-test");
        std::fs::create_dir_all(&dir).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(ensure_model_downloaded(&dir)).unwrap();
        assert!(is_model_downloaded(&dir));

        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();

        let query = generate_embedding(&dir, "deploy the staging server").unwrap();
        let related = generate_embedding(&dir, "push the build to the staging environment").unwrap();
        let unrelated = generate_embedding(&dir, "banana bread recipe").unwrap();

        assert_eq!(query.len(), EMBEDDING_DIM);
        // Normalised vectors make the dot product the cosine similarity.
        assert!(
            (dot(&query, &query) - 1.0).abs() < 1e-3,
            "embedding is not unit length"
        );

        let near = dot(&query, &related);
        let far = dot(&query, &unrelated);
        println!("related={near:.4} unrelated={far:.4}");
        assert!(
            near > far,
            "semantically related text must score higher ({near} vs {far})"
        );
    }
}
