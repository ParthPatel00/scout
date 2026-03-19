use std::path::PathBuf;

use anyhow::Result;

#[allow(dead_code)]
pub const EMBEDDING_DIM: usize = 768;
pub const MODEL_ID: &str = "microsoft/unixcoder-base";

// ─── Path helpers ──────────────────────────────────────────────────────────────

pub fn models_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("scout").join("models")
}

pub fn model_dir() -> PathBuf {
    models_dir().join("unixcoder-base")
}

pub fn is_model_downloaded() -> bool {
    model_dir().join("config.json").exists()
}

/// Download the UniXcoder model weights from HuggingFace.
///
/// Downloads only the files required for inference and shows a progress
/// indicator. Skips files that are already present on disk.
pub fn download_model() -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};

    let dest = model_dir();
    std::fs::create_dir_all(&dest)?;

    // Files required for inference — ordered smallest → largest so feedback
    // appears immediately before the long weights download begins.
    let files: &[(&str, &str)] = &[
        ("tokenizer_config.json", "tokenizer_config.json"),
        ("special_tokens_map.json", "special_tokens_map.json"),
        ("config.json",            "config.json"),
        ("vocab.json",             "vocab.json"),
        ("merges.txt",             "merges.txt"),
        ("tokenizer.json",         "tokenizer.json"),
        ("pytorch_model.bin",      "pytorch_model.bin"),
    ];

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;

    let base_url = format!("https://huggingface.co/{MODEL_ID}/resolve/main");

    for (remote, local) in files {
        let out = dest.join(local);
        if out.exists() {
            println!("  \x1b[2m{local} already present, skipping\x1b[0m");
            continue;
        }

        let url = format!("{base_url}/{remote}");
        let resp = client.get(&url).send()?;
        if !resp.status().is_success() {
            // Some models omit merges.txt — skip gracefully.
            if resp.status().as_u16() == 404 {
                continue;
            }
            anyhow::bail!("Download failed for {remote}: HTTP {}", resp.status());
        }

        let total = resp.content_length().unwrap_or(0);
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("  {msg:25} [{bar:40.cyan/blue}] {bytes}/{total_bytes}")?
                .progress_chars("=>-"),
        );
        pb.set_message(local.to_string());

        let mut file = std::fs::File::create(&out)?;
        let mut downloaded: u64 = 0;
        let mut reader = resp;
        loop {
            use std::io::Read;
            let mut buf = vec![0u8; 65536];
            let n = reader.read(&mut buf)?;
            if n == 0 { break; }
            use std::io::Write;
            file.write_all(&buf[..n])?;
            downloaded += n as u64;
            pb.set_position(downloaded);
        }
        pb.finish_and_clear();
        println!("  \x1b[32m✓\x1b[0m {local}");
    }

    println!("\x1b[32m✓\x1b[0m Model downloaded to {}", dest.display());
    Ok(())
}

/// Print instructions for obtaining the UniXcoder model weights (fallback).
pub fn print_download_instructions() {
    let path = model_dir();
    eprintln!("To enable semantic search, download the UniXcoder model (~350 MB):");
    eprintln!();
    eprintln!("  Option 1 — huggingface_hub (Python):");
    eprintln!("    pip install huggingface_hub");
    eprintln!("    python -c \"from huggingface_hub import snapshot_download; \\");
    eprintln!("      snapshot_download('{}', local_dir='{}')\"", MODEL_ID, path.display());
    eprintln!();
    eprintln!("  Option 2 — git-lfs:");
    eprintln!("    git clone https://huggingface.co/{} {}", MODEL_ID, path.display());
    eprintln!();
    eprintln!("After downloading, retry your command.");
}

// ─── Loader ────────────────────────────────────────────────────────────────────

/// Load the local UniXcoder model from disk.
/// Available only when compiled with `--features local-models`.
#[cfg(feature = "local-models")]
pub fn load_model() -> Result<Box<dyn super::EmbeddingModel>> {
    let path = model_dir();
    if !path.join("config.json").exists() {
        anyhow::bail!(
            "Model not found at {}.\nRun: scout index --download-model",
            path.display()
        );
    }
    let embedder = UnixcoderEmbedder::load(&path).context("failed to load UniXcoder model")?;
    Ok(Box::new(embedder))
}

/// Stub that always errors when the feature is disabled.
#[cfg(not(feature = "local-models"))]
pub fn load_model() -> Result<Box<dyn super::EmbeddingModel>> {
    anyhow::bail!(
        "Local model support is not compiled in.\n\
         Rebuild with: cargo build --features local-models"
    )
}

// ─── Candle-backed UniXcoder (local-models feature only) ──────────────────────

#[cfg(feature = "local-models")]
struct UnixcoderEmbedder {
    model: candle_transformers::models::bert::BertModel,
    tokenizer: tokenizers::Tokenizer,
    device: candle_core::Device,
}

#[cfg(feature = "local-models")]
impl UnixcoderEmbedder {
    fn load(dir: &std::path::Path) -> Result<Self> {
        use candle_core::{DType, Device};
        use candle_nn::VarBuilder;
        use candle_transformers::models::bert::{BertModel, Config};

        let device = Device::Cpu;

        let config_str = std::fs::read_to_string(dir.join("config.json"))
            .context("failed to read model config.json")?;
        let config: Config =
            serde_json::from_str(&config_str).context("failed to parse model config.json")?;

        // Accept either pytorch_model.bin or model.safetensors.
        let vb = {
            let pt = dir.join("pytorch_model.bin");
            if pt.exists() {
                VarBuilder::from_pth(&pt, DType::F32, &device)
                    .context("failed to load pytorch_model.bin")?
            } else {
                let st = dir.join("model.safetensors");
                unsafe {
                    VarBuilder::from_mmaped_safetensors(&[&st], DType::F32, &device)
                        .context("failed to load model.safetensors")?
                }
            }
        };

        let model = BertModel::load(vb, &config).context("failed to build BertModel")?;

        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("failed to load tokenizer: {e}"))?;

        Ok(Self { model, tokenizer, device })
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        use candle_core::Tensor;

        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenization failed: {e}"))?;

        // Truncate to model's maximum sequence length.
        let len = enc.get_ids().len().min(512);
        let ids: Vec<u32> = enc.get_ids()[..len].to_vec();
        let mask: Vec<u32> = enc.get_attention_mask()[..len].to_vec();
        let type_ids: Vec<u32> = enc.get_type_ids()[..len].to_vec();

        let ids_t = Tensor::new(&ids[..], &self.device)?.unsqueeze(0)?;
        let mask_t = Tensor::new(&mask[..], &self.device)?.unsqueeze(0)?;
        let type_t = Tensor::new(&type_ids[..], &self.device)?.unsqueeze(0)?;

        let output = self.model.forward(&ids_t, &type_t, Some(&mask_t))?;
        // Mean-pool over the sequence dimension, then remove the batch dim.
        Ok(output.mean(1)?.squeeze(0)?.to_vec1::<f32>()?)
    }
}

#[cfg(feature = "local-models")]
impl super::EmbeddingModel for UnixcoderEmbedder {
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed_one(t)).collect()
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIM
    }

    fn model_name(&self) -> &str {
        MODEL_ID
    }
}
