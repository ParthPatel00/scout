/// Compact vector store with per-vector scalar quantization (f32 → u8, 4× compression).
///
/// # File format  (.codesearch/vectors.bin)
///
/// ```text
/// [magic:   4 bytes = b"CVEC"]
/// [version: 4 bytes = u32 LE]
/// [dim:     4 bytes = u32 LE]
/// [n_vecs:  8 bytes = u64 LE]
/// [--- body, repeated n_vecs times ---]
/// [unit_id: 8 bytes = i64 LE]
/// [v_min:   4 bytes = f32 LE]
/// [v_max:   4 bytes = f32 LE]
/// [data:    dim bytes = u8 (quantized)]
/// ```
///
/// The file is memory-mapped for fast random read access.  A ZSTD-compressed
/// copy is also written alongside for portable backups.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const MAGIC: &[u8; 4] = b"CVEC";
const VERSION: u32 = 1;
/// Byte size of the file header.
const HEADER_SIZE: usize = 4 + 4 + 4 + 8; // 20 bytes
/// Byte overhead per record (before the quantized data).
const RECORD_OVERHEAD: usize = 8 + 4 + 4; // id + v_min + v_max

/// Maximum decoded-f32 vectors kept in the hot cache (~100 MB at dim=768).
const CACHE_CAPACITY: usize = 32_768;

// ─── Internal record ──────────────────────────────────────────────────────────

struct Record {
    unit_id: i64,
    v_min: f32,
    v_max: f32,
    /// Per-vector scalar-quantized bytes.
    data: Vec<u8>,
}

// ─── VectorStore ──────────────────────────────────────────────────────────────

pub struct VectorStore {
    path: PathBuf,
    dim: usize,
    records: Vec<Record>,
    id_index: HashMap<i64, usize>,
    /// Hot cache: decoded f32 vectors for recently searched IDs.
    hot_cache: HashMap<i64, Vec<f32>>,
}

impl VectorStore {
    /// Create a new, empty store.  Nothing is written to disk until `flush()`.
    pub fn new(path: &Path, dim: usize) -> Self {
        Self {
            path: path.to_path_buf(),
            dim,
            records: Vec::new(),
            id_index: HashMap::new(),
            hot_cache: HashMap::new(),
        }
    }

    /// Load an existing store using memory-mapped I/O.
    pub fn load(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("cannot open vector store at {}", path.display()))?;
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let bytes: &[u8] = &mmap;

        if bytes.len() < HEADER_SIZE {
            bail!("vector store file is too small ({}B)", bytes.len());
        }

        // Parse fixed header.
        if &bytes[0..4] != MAGIC {
            bail!("invalid vector store: wrong magic bytes");
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into()?);
        if version != VERSION {
            bail!("unsupported vector store version {version}");
        }
        let dim = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        let n_vecs = u64::from_le_bytes(bytes[12..20].try_into()?) as usize;

        let record_size = RECORD_OVERHEAD + dim;
        let expected = HEADER_SIZE + n_vecs * record_size;
        if bytes.len() < expected {
            bail!(
                "vector store truncated (expected {expected}B, got {}B)",
                bytes.len()
            );
        }

        let mut records = Vec::with_capacity(n_vecs);
        let mut id_index = HashMap::with_capacity(n_vecs);

        for i in 0..n_vecs {
            let off = HEADER_SIZE + i * record_size;
            let unit_id = i64::from_le_bytes(bytes[off..off + 8].try_into()?);
            let v_min = f32::from_le_bytes(bytes[off + 8..off + 12].try_into()?);
            let v_max = f32::from_le_bytes(bytes[off + 12..off + 16].try_into()?);
            let data = bytes[off + 16..off + 16 + dim].to_vec();
            id_index.insert(unit_id, records.len());
            records.push(Record { unit_id, v_min, v_max, data });
        }

        Ok(Self {
            path: path.to_path_buf(),
            dim,
            records,
            id_index,
            hot_cache: HashMap::new(),
        })
    }

    /// Insert or replace the embedding for `unit_id`.
    pub fn insert(&mut self, unit_id: i64, vector: &[f32]) -> Result<()> {
        if vector.len() != self.dim {
            bail!(
                "dimension mismatch: store expects {}, got {}",
                self.dim,
                vector.len()
            );
        }
        let (data, v_min, v_max) = quantize(vector);
        let record = Record { unit_id, v_min, v_max, data };

        if let Some(&idx) = self.id_index.get(&unit_id) {
            self.records[idx] = record;
        } else {
            let idx = self.records.len();
            self.id_index.insert(unit_id, idx);
            self.records.push(record);
        }
        // Invalidate any cached decode.
        self.hot_cache.remove(&unit_id);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Brute-force cosine-similarity search.
    /// Returns up to `top_k` (unit_id, score) pairs, sorted by descending score.
    pub fn search(&mut self, query: &[f32], top_k: usize) -> Result<Vec<(i64, f32)>> {
        if query.len() != self.dim {
            bail!("query dimension mismatch: expected {}, got {}", self.dim, query.len());
        }
        if self.records.is_empty() {
            return Ok(vec![]);
        }

        let q_norm = l2_norm(query);
        if q_norm < 1e-9 {
            return Ok(vec![]);
        }
        let q_unit: Vec<f32> = query.iter().map(|x| x / q_norm).collect();

        // Score phase: borrow records and cache as separate slices.
        let records = &self.records;
        let hot_cache = &self.hot_cache;

        let mut scores: Vec<(i64, f32)> = records
            .iter()
            .map(|rec| {
                let decoded = if let Some(cached) = hot_cache.get(&rec.unit_id) {
                    cached.clone()
                } else {
                    dequantize(&rec.data, rec.v_min, rec.v_max)
                };
                let norm = l2_norm(&decoded).max(1e-9);
                let score = dot_product(&q_unit, &decoded) / norm;
                (rec.unit_id, score)
            })
            .collect();

        scores.sort_unstable_by(|a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });
        scores.truncate(top_k);

        // Warm the hot cache for the top-k results.
        for (id, _) in &scores {
            if !self.hot_cache.contains_key(id) {
                if let Some(&idx) = self.id_index.get(id) {
                    let rec = &self.records[idx];
                    let decoded = dequantize(&rec.data, rec.v_min, rec.v_max);
                    if self.hot_cache.len() >= CACHE_CAPACITY {
                        // Evict an arbitrary entry.
                        if let Some(evict) = self.hot_cache.keys().next().cloned() {
                            self.hot_cache.remove(&evict);
                        }
                    }
                    self.hot_cache.insert(*id, decoded);
                }
            }
        }

        Ok(scores)
    }

    /// Write the store to disk.  Atomic rename guarantees no partial writes.
    /// Also writes a ZSTD-compressed copy alongside for portability.
    pub fn flush(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let n_vecs = self.records.len();
        let record_size = RECORD_OVERHEAD + self.dim;
        let mut buf = Vec::with_capacity(HEADER_SIZE + n_vecs * record_size);

        // Header.
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION.to_le_bytes());
        buf.extend_from_slice(&(self.dim as u32).to_le_bytes());
        buf.extend_from_slice(&(n_vecs as u64).to_le_bytes());

        // Records.
        for rec in &self.records {
            buf.extend_from_slice(&rec.unit_id.to_le_bytes());
            buf.extend_from_slice(&rec.v_min.to_le_bytes());
            buf.extend_from_slice(&rec.v_max.to_le_bytes());
            buf.extend_from_slice(&rec.data);
        }

        // Atomic write of the uncompressed mmap file.
        let tmp = self.path.with_extension("bin.tmp");
        std::fs::write(&tmp, &buf).context("failed to write vector store")?;
        std::fs::rename(&tmp, &self.path).context("failed to rename vector store")?;

        // Write ZSTD-compressed copy.
        let zst_path = self.path.with_extension("bin.zst");
        let compressed = zstd::encode_all(buf.as_slice(), 3)
            .context("failed to zstd-compress vector store")?;
        std::fs::write(&zst_path, compressed)
            .context("failed to write zstd vector store")?;

        Ok(())
    }
}

// ─── Quantization helpers ─────────────────────────────────────────────────────

/// Per-vector scalar quantization: map [v_min, v_max] → [0, 255].
fn quantize(vec: &[f32]) -> (Vec<u8>, f32, f32) {
    let v_min = vec.iter().cloned().fold(f32::INFINITY, f32::min);
    let v_max = vec.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = (v_max - v_min).max(1e-9);
    let data = vec
        .iter()
        .map(|&x| ((x - v_min) / range * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect();
    (data, v_min, v_max)
}

/// Inverse of `quantize`.
fn dequantize(data: &[u8], v_min: f32, v_max: f32) -> Vec<f32> {
    let range = v_max - v_min;
    data.iter().map(|&b| v_min + b as f32 / 255.0 * range).collect()
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip_insert_and_search() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vectors.bin");
        let dim = 4;
        let mut store = VectorStore::new(&path, dim);

        let v1 = vec![1.0f32, 0.0, 0.0, 0.0];
        let v2 = vec![0.0f32, 1.0, 0.0, 0.0];
        let v3 = vec![0.95f32, 0.1, 0.0, 0.0]; // close to v1

        store.insert(1, &v1).unwrap();
        store.insert(2, &v2).unwrap();
        store.insert(3, &v3).unwrap();

        let results = store.search(&v1, 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 1); // exact match first
        assert!(results[0].1 > 0.99);
    }

    #[test]
    fn flush_and_reload() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vectors.bin");
        let dim = 8;
        let mut store = VectorStore::new(&path, dim);

        let v = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        store.insert(42, &v).unwrap();
        store.flush().unwrap();

        let mut loaded = VectorStore::load(&path).unwrap();
        assert_eq!(loaded.len(), 1);

        let results = loaded.search(&v, 1).unwrap();
        assert_eq!(results[0].0, 42);
        assert!(results[0].1 > 0.98);
    }

    #[test]
    fn update_existing_entry() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("vectors.bin");
        let dim = 4;
        let mut store = VectorStore::new(&path, dim);

        store.insert(1, &[1.0, 0.0, 0.0, 0.0]).unwrap();
        store.insert(1, &[0.0, 1.0, 0.0, 0.0]).unwrap(); // overwrite
        assert_eq!(store.len(), 1);
    }
}
