# CodeSearch - Production-Ready Technical Deep Dive
## Critical Analysis of Every Design Decision for 10k+ Users

**Last Updated:** March 18, 2026  
**Status:** Pre-Production Technical Review

---

## Executive Summary

This document provides a **critical analysis** of every technical decision in the CodeSearch project, identifying potential failure modes, scalability issues, and production risks. If this tool will be used by tens of thousands of engineers, **we cannot afford to get this wrong**.

**Key Questions We Must Answer:**
1. Will it scale to millions of functions across hundreds of repositories?
2. Will file watching crash on large monorepos?
3. Will vector databases corrupt or become too large?
4. Will embedding generation block users for hours?
5. Will cross-platform compatibility issues plague users?
6. Will the tool consume all available RAM/disk?
7. Will concurrent access cause race conditions?
8. Will breaking changes force users to re-index everything?

---

## Table of Contents

1. [Vector Database Scaling Issues](#vector-database-scaling-issues)
2. [File Watcher Catastrophic Failures](#file-watcher-catastrophic-failures)
3. [Tree-sitter Performance & Memory Problems](#tree-sitter-performance--memory-problems)
4. [Embedding Generation Bottlenecks](#embedding-generation-bottlenecks)
5. [Storage Explosion & Disk Space](#storage-explosion--disk-space)
6. [Cross-Platform Compatibility Nightmares](#cross-platform-compatibility-nightmares)
7. [Concurrency & Race Conditions](#concurrency--race-conditions)
8. [Index Corruption & Data Loss](#index-corruption--data-loss)
9. [Migration & Breaking Changes](#migration--breaking-changes)
10. [Network & API Rate Limits](#network--api-rate-limits)
11. [Security & Privacy Concerns](#security--privacy-concerns)
12. [Performance Degradation Over Time](#performance-degradation-over-time)
13. [Revised Architecture](#revised-architecture)

---

## 1. Vector Database Scaling Issues

### Problem: Embedded Qdrant Won't Scale

**Research Findings:**
- Embedded vector databases like Qdrant struggle beyond ~10M vectors without proper sharding
- At billions of vectors, HNSW graphs must reside entirely in memory, making it expensive to scale
- Memory footprint: 1024-dim embedding = 4KB per vector × 3 (replication) = 12KB per vector

**Real-World Scale:**
- Large monorepo: 100,000 functions
- 10 repositories: 1,000,000 functions
- 100 users × 10 repos each: 10,000,000 functions

**Memory Requirements (Unoptimized):**
- 10M functions × 768 dims × 4 bytes (f32) = **30 GB RAM minimum**
- HNSW index overhead: +50% = **45 GB RAM**
- This is **PER USER** if we don't share indexes!

### Critical Questions

**Q1: Will embedded Qdrant handle 10M+ vectors on a laptop?**
**A:** **NO.** Embedded Qdrant on a single machine requires 300-800 GiB memory for billion-scale datasets with HNSW

**Q2: What happens when a user runs out of RAM?**
**A:** **Disk swapping → 100x slower queries → Tool becomes unusable**

**Q3: Can we use Qdrant's disk-backed mode?**
**A:** Yes, but query latency increases from 40ms to 400ms when offloading to SSD

### Solutions

#### **Option 1: Product Quantization (PQ) - MANDATORY**

```rust
// Instead of storing full f32 vectors
struct FullVector {
    data: Vec<f32>, // 768 * 4 bytes = 3,072 bytes
}

// Use Product Quantization
struct QuantizedVector {
    codebook_ids: Vec<u8>, // 768 / 8 subspaces * 1 byte = 96 bytes
    residuals: Vec<u8>,     // Optional refinement
}

// Reduction: 3,072 bytes → 96 bytes = 32x compression!
```

Product Quantization can reduce memory footprint by more than an order of magnitude

**Implementation:**
- Use FAISS IVF-PQ or Qdrant's scalar quantization
- **Accuracy loss:** ~2-5% recall at k=10
- **Acceptable trade-off** for code search (we're searching, not matching exactly)

#### **Option 2: Inverted File Index (IVF) - REQUIRED**

IVF narrows search by pre-clustering vectors, only searching relevant partitions

```rust
// Cluster 1M vectors into 1000 clusters
// Search only probes 10 clusters (1% of data)
// 100x speedup on search, minimal memory overhead

struct IVFIndex {
    clusters: Vec<Centroid>,      // 1000 clusters × 768 dims = 3MB
    inverted_lists: Vec<Vec<u32>>, // Pointers to vectors in each cluster
}
```

#### **Option 3: Disk-Based Storage with Mmap - REQUIRED**

```rust
use memmap2::Mmap;

struct DiskBackedVectorDB {
    // Memory-mapped file
    mmap: Mmap,
    
    // In-memory index (small)
    ivf_index: IVFIndex,
    
    // Read vectors on-demand
    fn get_vector(&self, id: u64) -> Vec<f32> {
        let offset = id * VECTOR_SIZE_BYTES;
        decompress(&self.mmap[offset..offset + COMPRESSED_SIZE])
    }
}
```

**Benefits:**
- OS handles paging (virtual memory)
- Only active vectors in RAM
- Works well with SSD for hybrid in-memory/on-disk architecture

### Revised Vector DB Architecture

```rust
pub struct ScalableVectorDB {
    // Tier 1: In-memory hot cache (most recently used)
    hot_cache: LRUCache<u64, Vec<f32>>, // ~100 MB
    
    // Tier 2: Compressed vectors on disk (mmap)
    cold_storage: Mmap, // All vectors, PQ-compressed
    
    // Tier 3: IVF index for fast lookup
    ivf_index: IVFIndex, // ~5-10 MB
    
    // Metadata
    metadata_db: SqliteConnection,
}

impl ScalableVectorDB {
    pub fn search(&self, query: &[f32], k: usize) -> Vec<SearchResult> {
        // 1. IVF: Find relevant clusters (1% of data)
        let cluster_ids = self.ivf_index.search_clusters(query, nprobe=10);
        
        // 2. Load vectors from clusters (mmap, on-demand)
        let mut candidates = Vec::new();
        for cluster_id in cluster_ids {
            let vector_ids = self.ivf_index.get_cluster_vectors(cluster_id);
            for id in vector_ids {
                // Check hot cache first
                let vec = self.hot_cache.get(&id)
                    .or_else(|| Some(self.load_from_disk(id)));
                candidates.push((id, cosine_similarity(query, &vec)));
            }
        }
        
        // 3. Top-k heap
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        candidates.truncate(k);
        
        // 4. Update hot cache
        for (id, _) in &candidates {
            self.hot_cache.insert(*id, self.load_from_disk(*id));
        }
        
        candidates
    }
}
```

**Memory Footprint:**
- Hot cache: 100 MB
- IVF index: 10 MB
- Compressed vectors: 0 (on disk, mmap)
- **Total RAM: ~110 MB** (vs 45 GB!)

---

## 2. File Watcher Catastrophic Failures

### Problem: File Watchers Don't Scale

**Research Findings:**
- File watcher initialization took 11 minutes for 883 libraries with semanticdb enabled
- Large codebases hit "too many open files" error on Linux
- Linux has inotify limits: default 8192 watches per user
- 36,000 files (including node_modules) exhausts file watchers
- File watcher consumes 100% CPU in large TypeScript projects
- Chokidar (popular Node.js watcher) performs poorly on large folders

### Critical Questions

**Q1: What happens when a user has 50,000 files?**
**A:** **File watcher crashes or consumes all CPU**

**Q2: What about node_modules, .git, target/ directories?**
**A:** **These alone can have 100,000+ files → instant failure**

**Q3: Can we increase inotify limits?**
**A:** **Requires sudo, not acceptable for CLI tool**

### Real-World Failure Scenarios

```bash
# Typical monorepo structure
monorepo/
├── node_modules/        # 100,000 files
├── .git/                # 50,000 files
├── target/              # 20,000 build artifacts
├── dist/                # 10,000 files
├── coverage/            # 5,000 files
└── src/                 # 2,000 actual source files

# Total: 187,000 files
# inotify limit: 8,192 watches
# Result: CRASH
```

### Solutions

#### **Option 1: Smart Filtering - MANDATORY**

```rust
const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "out",
    "bin",
    "obj",
    ".next",
    ".nuxt",
    "vendor",
    "__pycache__",
    ".pytest_cache",
    ".venv",
    "venv",
    ".tox",
    "coverage",
    ".nyc_output",
    "*.min.js",
    "*.bundle.js",
];

// Only watch source directories
fn should_watch(path: &Path) -> bool {
    let path_str = path.to_str().unwrap();
    
    // Exclude common junk directories
    if DEFAULT_EXCLUDES.iter().any(|ex| path_str.contains(ex)) {
        return false;
    }
    
    // Only watch known source file extensions
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "py" | "js" | "ts" | "go" | "java" | "cpp" | "c" | "h")
    )
}
```

**Reduction:**
- 187,000 files → **2,000 watched files** (93% reduction)
- Well within inotify limits

#### **Option 2: Polling Fallback - REQUIRED**

For systems where file watching fails, use polling with FilePoller

```rust
pub enum WatchStrategy {
    Native(notify::RecommendedWatcher), // Try first
    Polling(PollingWatcher),             // Fallback
}

impl WatchStrategy {
    pub fn new(repo_path: &Path) -> Self {
        match notify::recommended_watcher(handler) {
            Ok(watcher) => {
                // Check if it actually works
                if test_file_watch(&watcher, repo_path) {
                    WatchStrategy::Native(watcher)
                } else {
                    warn!("File watching unavailable, using polling");
                    WatchStrategy::Polling(PollingWatcher::new(repo_path))
                }
            }
            Err(_) => {
                warn!("File watching unavailable, using polling");
                WatchStrategy::Polling(PollingWatcher::new(repo_path))
            }
        }
    }
}

struct PollingWatcher {
    last_scan: HashMap<PathBuf, SystemTime>,
    poll_interval: Duration, // 5 seconds
}

impl PollingWatcher {
    fn poll(&mut self) -> Vec<PathBuf> {
        let mut changed_files = Vec::new();
        
        for entry in WalkDir::new(&self.repo_path)
            .into_iter()
            .filter_entry(|e| should_watch(e.path()))
        {
            let path = entry.path();
            let modified = fs::metadata(path).modified().unwrap();
            
            if let Some(last_mod) = self.last_scan.get(path) {
                if modified > *last_mod {
                    changed_files.push(path.to_path_buf());
                }
            }
            
            self.last_scan.insert(path.to_path_buf(), modified);
        }
        
        changed_files
    }
}
```

#### **Option 3: Git-Based Watching - BEST SOLUTION**

**Key Insight:** Instead of watching filesystem, watch git!

```rust
// Don't watch 187,000 files
// Watch git index instead (1 file!)

struct GitWatcher {
    repo: git2::Repository,
    last_commit: Oid,
}

impl GitWatcher {
    fn check_for_changes(&mut self) -> Vec<PathBuf> {
        let head = self.repo.head().unwrap().peel_to_commit().unwrap();
        
        if head.id() != self.last_commit {
            // Commit changed, get diff
            let old_tree = self.repo.find_commit(self.last_commit)
                .unwrap()
                .tree()
                .unwrap();
            let new_tree = head.tree().unwrap();
            
            let diff = self.repo.diff_tree_to_tree(
                Some(&old_tree),
                Some(&new_tree),
                None
            ).unwrap();
            
            let changed_files = extract_changed_files(diff);
            self.last_commit = head.id();
            
            changed_files
        } else {
            // Check for unstaged changes
            let diff = self.repo.diff_index_to_workdir(None, None).unwrap();
            extract_changed_files(diff)
        }
    }
}
```

**Benefits:**
- Only 1 file to watch (.git/index or .git/HEAD)
- Git already tracks what changed
- **Works with 1M+ files in repo**
- No inotify limits

**Drawback:**
- Misses changes not in git (new files before `git add`)
- **Solution:** Hybrid approach - poll new files every 10s

### Revised File Watching Architecture

```rust
pub struct FileWatcher {
    // Primary strategy
    strategy: WatchStrategy,
    
    // Queue for incremental updates
    change_queue: Arc<Mutex<VecDeque<PathBuf>>>,
    
    // Worker thread
    worker: Option<JoinHandle<()>>,
}

pub enum WatchStrategy {
    Git(GitWatcher),           // Best (works with millions of files)
    Native(notify::Watcher),   // Good (works with <10k files)
    Polling(PollingWatcher),   // Fallback (always works)
    Disabled,                  // Manual updates only
}

impl FileWatcher {
    pub fn new(repo_path: &Path) -> Self {
        // Try strategies in order
        let strategy = if is_git_repo(repo_path) {
            WatchStrategy::Git(GitWatcher::new(repo_path))
        } else if can_use_native_watcher() {
            WatchStrategy::Native(create_native_watcher(repo_path))
        } else {
            WatchStrategy::Polling(PollingWatcher::new(repo_path))
        };
        
        Self {
            strategy,
            change_queue: Arc::new(Mutex::new(VecDeque::new())),
            worker: None,
        }
    }
    
    pub fn start(&mut self) {
        let queue = Arc::clone(&self.change_queue);
        let strategy = self.strategy.clone();
        
        self.worker = Some(std::thread::spawn(move || {
            loop {
                // Check for changes (frequency depends on strategy)
                let changed_files = strategy.check();
                
                // Queue changes
                let mut q = queue.lock().unwrap();
                for file in changed_files {
                    q.push_back(file);
                }
                
                // Sleep based on strategy
                match strategy {
                    WatchStrategy::Git(_) => sleep(Duration::from_secs(5)),
                    WatchStrategy::Native(_) => sleep(Duration::from_millis(100)),
                    WatchStrategy::Polling(_) => sleep(Duration::from_secs(10)),
                    _ => break,
                }
            }
        }));
    }
}
```

---

## 3. Tree-sitter Performance & Memory Problems

### Problem: Tree-sitter Can Be Slow and Memory-Heavy

**Research Findings:**
- Tree-sitter had a memory leak in CallbackInput class that consumed all memory
- Tree-sitter-haskell was 50x slower due to excessive malloc calls in parser combinators
- VS Code dropped TextMate for Tree-sitter but files >100KB still caused performance issues
- Neovim had to implement async parsing because tree-sitter blocked user input on large files

### Critical Questions

**Q1: What happens when parsing a 10,000-line file?**
**A:** Tree-sitter can hang/block for seconds on large files without async parsing

**Q2: What about minified JavaScript files?**
**A:** **Disaster.** Single-line 1MB files cause tree-sitter to choke

**Q3: Memory leaks?**
**A:** Yes, documented memory leaks in node-tree-sitter's CallbackInput

### Real-World Failure Scenarios

```javascript
// minified.js - 50,000 lines in 1 file, 5 MB
// Tree-sitter tries to parse entire file
// Result: 30+ seconds, 2 GB RAM, UI frozen
```

### Solutions

#### **Option 1: File Size Limits - MANDATORY**

```rust
const MAX_FILE_SIZE: u64 = 1_000_000; // 1 MB
const MAX_LINES: usize = 10_000;

fn should_parse_file(path: &Path) -> Result<bool> {
    let metadata = fs::metadata(path)?;
    
    // Skip huge files
    if metadata.len() > MAX_FILE_SIZE {
        warn!("Skipping {}: file too large ({})", path.display(), metadata.len());
        return Ok(false);
    }
    
    // Check line count (cheap)
    let lines = BufReader::new(File::open(path)?)
        .lines()
        .count();
    
    if lines > MAX_LINES {
        warn!("Skipping {}: too many lines ({})", path.display(), lines);
        return Ok(false);
    }
    
    Ok(true)
}
```

#### **Option 2: Async Parsing - REQUIRED**

Neovim implemented async parsing to prevent blocking on large files

```rust
use tokio::task;

pub async fn parse_file_async(path: &Path) -> Result<Tree> {
    let source = fs::read_to_string(path)?;
    
    // Parse in separate thread pool (blocking task)
    let tree = task::spawn_blocking(move || {
        let mut parser = Parser::new();
        parser.set_language(language)?;
        parser.parse(&source, None)
    }).await??;
    
    Ok(tree)
}

// Batch parsing with concurrency limit
pub async fn parse_files_batch(files: Vec<PathBuf>) -> Vec<Result<Tree>> {
    use futures::stream::{self, StreamExt};
    
    stream::iter(files)
        .map(|path| parse_file_async(&path))
        .buffer_unordered(num_cpus::get()) // Parallel, but limited
        .collect()
        .await
}
```

#### **Option 3: Timeout + Fallback - CRITICAL**

```rust
use tokio::time::timeout;

async fn parse_with_timeout(path: &Path) -> Result<Option<Tree>> {
    match timeout(
        Duration::from_secs(10),
        parse_file_async(path)
    ).await {
        Ok(Ok(tree)) => Ok(Some(tree)),
        Ok(Err(e)) => {
            error!("Parse error for {}: {}", path.display(), e);
            Ok(None)
        }
        Err(_) => {
            error!("Parse timeout for {}", path.display());
            // Fallback: regex-based extraction
            Ok(None)
        }
    }
}
```

#### **Option 4: Incremental Parsing - OPTIMIZATION**

Tree-sitter supports incremental parsing (reuse old tree):

```rust
struct FileParser {
    old_trees: HashMap<PathBuf, Tree>,
}

impl FileParser {
    fn parse_incremental(&mut self, path: &Path, source: &str) -> Result<Tree> {
        let old_tree = self.old_trees.get(path);
        
        let mut parser = Parser::new();
        parser.set_language(language)?;
        
        // Reuse old tree (tree-sitter does smart diffing)
        let tree = parser.parse(source, old_tree)?;
        
        self.old_trees.insert(path.to_path_buf(), tree.clone());
        Ok(tree)
    }
}
```

**Performance:**
- Full parse: 500ms for 5000-line file
- Incremental parse (10 lines changed): **20ms** (25x faster!)

### Revised Tree-sitter Architecture

```rust
pub struct RobustParser {
    parsers: HashMap<Language, Parser>,
    old_trees: HashMap<PathBuf, Tree>,
    semaphore: Arc<Semaphore>, // Limit concurrent parses
}

impl RobustParser {
    pub async fn parse_file(&mut self, path: &Path) -> Result<Option<ParsedFile>> {
        // 1. Check file size
        if !should_parse_file(path)? {
            return Ok(None);
        }
        
        // 2. Acquire semaphore (limit concurrency)
        let _permit = self.semaphore.acquire().await?;
        
        // 3. Read file
        let source = fs::read_to_string(path)?;
        
        // 4. Parse with timeout
        match timeout(
            Duration::from_secs(10),
            self.parse_incremental(path, &source)
        ).await {
            Ok(Ok(tree)) => Ok(Some(ParsedFile { tree, source })),
            Ok(Err(e)) => {
                warn!("Parse error: {}", e);
                Ok(None)
            }
            Err(_) => {
                warn!("Parse timeout: {}", path.display());
                Ok(None) // Skip this file
            }
        }
    }
}
```

---

## 4. Embedding Generation Bottlenecks

### Problem: Embedding Generation is SLOW

**Research Findings:**
- UniXcoder: ~300ms per function on CPU
- 10,000 functions = **50 minutes** initial index
- Notion reduced embedding costs by 90% by moving from API to self-hosted Ray clusters

### Critical Questions

**Q1: Will users wait 50 minutes for initial index?**
**A:** **NO. Unacceptable.**

**Q2: What about incremental updates?**
**A:** 1 file = 10 functions = 3 seconds (acceptable)

**Q3: Can we use GPU?**
**A:** Most users don't have CUDA/Metal setup

**Q4: Batch size optimization?**
**A:** Batching 32-64 items reduces time from 50min to 5min

### Solutions

#### **Option 1: Aggressive Batching - MANDATORY**

```rust
const BATCH_SIZE: usize = 64; // Optimal for most models

async fn generate_embeddings_batch(
    functions: Vec<CodeUnit>,
    model: &EmbeddingModel
) -> Result<Vec<Vec<f32>>> {
    // Split into batches
    let batches: Vec<_> = functions.chunks(BATCH_SIZE).collect();
    
    let mut all_embeddings = Vec::new();
    
    for (i, batch) in batches.iter().enumerate() {
        // Show progress
        println!("Batch {}/{}", i + 1, batches.len());
        
        // Generate batch
        let signatures: Vec<_> = batch.iter()
            .map(|f| f.full_signature.clone())
            .collect();
        
        let embeddings = model.encode_batch(&signatures).await?;
        all_embeddings.extend(embeddings);
    }
    
    Ok(all_embeddings)
}
```

**Performance:**
- Sequential: 10,000 × 300ms = 50 minutes
- Batched (64): 10,000 / 64 × 300ms = **5 minutes** (10x faster!)

#### **Option 2: Parallel Batching - OPTIMIZATION**

```rust
use rayon::prelude::*;

async fn generate_embeddings_parallel(
    functions: Vec<CodeUnit>,
    model: Arc<EmbeddingModel>
) -> Result<Vec<Vec<f32>>> {
    let batches: Vec<_> = functions
        .chunks(BATCH_SIZE)
        .map(|c| c.to_vec())
        .collect();
    
    // Process batches in parallel (if multiple GPUs or CPUs)
    let embeddings: Vec<_> = batches
        .into_par_iter()
        .map(|batch| {
            let model = Arc::clone(&model);
            tokio::runtime::Handle::current().block_on(async move {
                let sigs: Vec<_> = batch.iter().map(|f| f.full_signature.clone()).collect();
                model.encode_batch(&sigs).await
            })
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    
    Ok(embeddings)
}
```

**Performance:**
- 4-core CPU: 5 minutes → **~90 seconds** (4x speedup if model allows parallel inference)

#### **Option 3: Background Indexing - UX CRITICAL**

```rust
pub struct BackgroundIndexer {
    queue: Arc<Mutex<VecDeque<PathBuf>>>,
    status: Arc<RwLock<IndexStatus>>,
}

pub struct IndexStatus {
    pub total_files: usize,
    pub indexed_files: usize,
    pub current_file: Option<PathBuf>,
    pub eta_seconds: u64,
}

impl BackgroundIndexer {
    pub fn start(&self) {
        let queue = Arc::clone(&self.queue);
        let status = Arc::clone(&self.status);
        
        tokio::spawn(async move {
            while let Some(file) = queue.lock().unwrap().pop_front() {
                // Update status
                {
                    let mut s = status.write().unwrap();
                    s.current_file = Some(file.clone());
                    s.eta_seconds = estimate_eta(&s);
                }
                
                // Index file
                if let Err(e) = index_file(&file).await {
                    error!("Failed to index {}: {}", file.display(), e);
                }
                
                // Update progress
                {
                    let mut s = status.write().unwrap();
                    s.indexed_files += 1;
                }
            }
        });
    }
    
    pub fn get_status(&self) -> IndexStatus {
        self.status.read().unwrap().clone()
    }
}

// CLI shows progress
$ codesearch index

Indexing repository...
[████████░░░░░░░░░░░░] 42% (2,453 / 5,821 functions)
Current: src/parser/typescript.rs
ETA: 2m 15s

Press Ctrl+C to stop (index will resume later)
```

**User Experience:**
- Non-blocking background indexing
- Can start searching immediately (partial results)
- Progress bar with ETA
- Resumable (save state to disk)

#### **Option 4: Lazy Embedding - SMART OPTIMIZATION**

**Key Insight:** Not all code needs embeddings!

```rust
pub enum EmbeddingStrategy {
    Eager,  // Embed everything immediately
    Lazy,   // Embed only when searched
    Hybrid, // Embed frequently accessed code
}

impl LazyEmbedder {
    fn on_search(&mut self, query: &str) -> Vec<SearchResult> {
        // 1. Fast search (AST + BM25) first
        let fast_results = self.ast_search(query);
        
        // 2. Check if top results have embeddings
        let missing_embeddings: Vec<_> = fast_results
            .iter()
            .filter(|r| !self.has_embedding(r.id))
            .collect();
        
        // 3. Generate missing embeddings on-demand (background)
        if !missing_embeddings.is_empty() {
            tokio::spawn(async move {
                generate_embeddings(missing_embeddings).await
            });
        }
        
        // 4. Return fast results immediately
        // Next search will use embeddings
        fast_results
    }
}
```

**Benefits:**
- First search: 100ms (AST + BM25 only)
- Subsequent searches: 200ms (with embeddings)
- Only embeds code that's actually searched
- **90% of code never needs embeddings!**

---

## 5. Storage Explosion & Disk Space

### Problem: Vector Embeddings are HUGE

**Math:**
- 10,000 functions × 768 dims × 4 bytes = **30 MB** (uncompressed)
- 100 repos × 30 MB = **3 GB**
- Plus metadata, BM25 index, git history = **5+ GB**

**User Impact:**
- MacBook Air 128 GB SSD → 5 GB is 4% of disk
- Multiple projects → **10-20 GB** disk usage
- Users will uninstall if it consumes too much space

### Solutions

#### **Option 1: Aggressive Quantization - MANDATORY**

```rust
// f32 → u8 quantization (4x reduction)
fn quantize_vector(vec: &[f32]) -> (Vec<u8>, QuantizationParams) {
    let min = vec.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = vec.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = max - min;
    
    let quantized: Vec<u8> = vec.iter()
        .map(|&v| ((v - min) / range * 255.0) as u8)
        .collect();
    
    (quantized, QuantizationParams { min, max })
}

// 768 dims × 4 bytes = 3,072 bytes → 768 bytes (4x smaller)
```

**Accuracy Impact:** Scalar quantization yields 63.7% storage reduction with minimal quality loss

#### **Option 2: Product Quantization - ADVANCED**

Product Quantization combined with dimension reduction yields 77.9% storage reduction

```rust
// Split 768-dim vector into 96 subspaces of 8 dims each
// Each subspace gets quantized to 256 centroids (1 byte)
// Result: 768 dims → 96 bytes (32x compression!)

struct PQEncoder {
    codebooks: Vec<Vec<Vec<f32>>>, // 96 codebooks × 256 centroids × 8 dims
}

impl PQEncoder {
    fn encode(&self, vec: &[f32]) -> Vec<u8> {
        let mut codes = Vec::with_capacity(96);
        
        for (i, subvec) in vec.chunks(8).enumerate() {
            // Find nearest centroid in this codebook
            let code = self.codebooks[i]
                .iter()
                .enumerate()
                .min_by(|(_, c1), (_, c2)| {
                    distance(subvec, c1).partial_cmp(&distance(subvec, c2)).unwrap()
                })
                .map(|(idx, _)| idx as u8)
                .unwrap();
            
            codes.push(code);
        }
        
        codes
    }
}

// Storage: 30 MB → 0.9 MB (33x reduction!)
```

#### **Option 3: Compression - SIMPLE WIN**

```rust
use zstd::stream::encode_all;

fn compress_vectors(vectors: &[Vec<f32>]) -> Vec<u8> {
    let bytes: Vec<u8> = vectors
        .iter()
        .flat_map(|v| {
            v.iter().flat_map(|f| f.to_le_bytes())
        })
        .collect();
    
    // ZSTD compression (level 3)
    encode_all(&bytes[..], 3).unwrap()
}

// Typical compression ratio: 2-3x
// 30 MB → 10-15 MB
```

#### **Option 4: Deduplication - SMART OPTIMIZATION**

```rust
// Many similar functions have nearly identical embeddings
// Example: getter/setter methods

struct DeduplicatedVectorDB {
    unique_vectors: Vec<Vec<f32>>,
    vector_index: HashMap<u64, usize>, // code_unit_id → unique_vector_idx
}

impl DeduplicatedVectorDB {
    fn add_vector(&mut self, id: u64, vec: Vec<f32>) {
        // Check if similar vector exists (cosine similarity > 0.99)
        if let Some(idx) = self.find_similar(&vec, 0.99) {
            // Reuse existing vector
            self.vector_index.insert(id, idx);
        } else {
            // Add new unique vector
            let idx = self.unique_vectors.len();
            self.unique_vectors.push(vec);
            self.vector_index.insert(id, idx);
        }
    }
}

// Typical deduplication: 20-30% reduction
// 10,000 functions → 7,000 unique vectors
```

### Combined Storage Optimization

```rust
pub struct OptimizedVectorStorage {
    // 1. Deduplicate first
    unique_vectors: Vec<Vec<f32>>, // ~7,000 unique out of 10,000
    
    // 2. Product Quantization
    pq_codes: Vec<Vec<u8>>, // 768 dims → 96 bytes each
    
    // 3. Compression
    compressed_data: Vec<u8>, // ZSTD compressed
    
    // 4. Metadata
    vector_map: HashMap<u64, usize>,
}

// Final storage:
// 10,000 functions × 768 dims × 4 bytes = 30 MB (original)
// → 7,000 unique (dedup) = 21 MB
// → 96 bytes each (PQ) = 672 KB
// → ZSTD compress = 220 KB (136x reduction!)
```

**Real-World Numbers:**
- 100 repos × 220 KB = **22 MB total** (vs 3 GB!)
- Acceptable for any laptop

---

## 6. Cross-Platform Compatibility Nightmares

### Problem: Rust Binary Distribution is Hard

**Challenges:**
1. Different platforms: Linux (x86_64, ARM64), macOS (Intel, Apple Silicon), Windows
2. Different libc: glibc (Linux), musl (Alpine), msvcrt (Windows)
3. Library dependencies: OpenSSL, sqlite, tree-sitter grammars
4. Code signing (macOS notarization, Windows SmartScreen)

### Critical Questions

**Q1: How do we distribute binaries?**
**A:** GitHub Releases with CI-built binaries for each platform

**Q2: What about library dependencies?**
**A:** Static linking + bundled resources

**Q3: macOS Gatekeeper blocking unsigned binaries?**
**A:** Code signing + notarization (costs $99/year Apple Developer account)

### Solutions

#### **Option 1: Static Linking - MANDATORY**

```toml
# Cargo.toml
[dependencies]
# Link SQLite statically
rusqlite = { version = "0.31", features = ["bundled"] }

# Link OpenSSL statically (if needed)
openssl = { version = "0.10", features = ["vendored"] }

# Embed tree-sitter grammars
tree-sitter-python = "0.20"
# ... compile grammars into binary
```

```rust
// build.rs - Embed grammars at compile time
use std::env;
use std::path::PathBuf;

fn main() {
    let grammars = &[
        "tree-sitter-python",
        "tree-sitter-rust",
        "tree-sitter-javascript",
        // ...
    ];
    
    for grammar in grammars {
        // Compile grammar
        cc::Build::new()
            .file(format!("{}/src/parser.c", grammar))
            .compile(grammar);
    }
}
```

**Result:** Single binary, no dependencies

#### **Option 2: CI/CD for Multi-Platform Builds**

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: codesearch-linux-x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-musl
            artifact: codesearch-linux-x86_64-musl
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            artifact: codesearch-linux-arm64
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact: codesearch-macos-x86_64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact: codesearch-macos-arm64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: codesearch-windows-x86_64.exe
    
    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      
      - name: Rename binary
        run: mv target/${{ matrix.target }}/release/codesearch* ${{ matrix.artifact }}
      
      - name: Upload artifact
        uses: actions/upload-artifact@v3
        with:
          name: ${{ matrix.artifact }}
          path: ${{ matrix.artifact }}
```

#### **Option 3: Installation Methods**

```bash
# 1. Homebrew (macOS/Linux)
brew install codesearch

# 2. Cargo
cargo install codesearch

# 3. Direct download
curl -sSL https://codesearch.dev/install.sh | sh

# 4. Package managers
# apt (Ubuntu/Debian)
sudo apt install codesearch

# yum (RHEL/CentOS)
sudo yum install codesearch

# Chocolatey (Windows)
choco install codesearch

# Scoop (Windows)
scoop install codesearch
```

---

## 7. Concurrency & Race Conditions

### Problem: Multiple Processes/Threads Accessing Same Index

**Scenarios:**
1. User runs `codesearch` while daemon is indexing
2. Two terminal windows searching simultaneously
3. IDE plugin + CLI tool both accessing index
4. File watcher updating while search is running

### Critical Questions

**Q1: What happens if search reads while index is being written?**
**A:** **Corrupted results or crash**

**Q2: What about SQLite concurrent access?**
**A:** SQLite has WAL mode, but still needs proper locking

**Q3: Vector DB corruption?**
**A:** **Very possible if not using transactions**

### Solutions

#### **Option 1: File-Based Locking - MANDATORY**

```rust
use fs2::FileExt;

pub struct IndexLock {
    lock_file: File,
}

impl IndexLock {
    pub fn acquire_read(repo_path: &Path) -> Result<Self> {
        let lock_path = repo_path.join(".codesearch/index.lock");
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(lock_path)?;
        
        // Shared lock (multiple readers allowed)
        lock_file.lock_shared()?;
        
        Ok(Self { lock_file })
    }
    
    pub fn acquire_write(repo_path: &Path) -> Result<Self> {
        let lock_path = repo_path.join(".codesearch/index.lock");
        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(lock_path)?;
        
        // Exclusive lock (only one writer)
        lock_file.lock_exclusive()?;
        
        Ok(Self { lock_file })
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

// Usage
pub fn search(query: &str) -> Result<Vec<SearchResult>> {
    let _lock = IndexLock::acquire_read(&repo_path)?;
    // ... perform search
    Ok(results)
}

pub fn index_file(path: &Path) -> Result<()> {
    let _lock = IndexLock::acquire_write(&repo_path)?;
    // ... update index
    Ok(())
}
```

#### **Option 2: SQLite WAL Mode + Proper Transactions**

```rust
pub fn init_sqlite_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    
    // Enable WAL mode (Write-Ahead Logging)
    // Allows concurrent readers + 1 writer
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        PRAGMA cache_size=-64000; -- 64 MB cache
        PRAGMA temp_store=MEMORY;
        PRAGMA mmap_size=30000000000; -- 30 GB mmap
    ")?;
    
    Ok(conn)
}

pub fn update_index_transaction(conn: &Connection, updates: Vec<Update>) -> Result<()> {
    // Use transaction for atomicity
    let tx = conn.transaction()?;
    
    for update in updates {
        tx.execute(
            "INSERT OR REPLACE INTO code_units (...) VALUES (...)",
            params![...],
        )?;
    }
    
    tx.commit()?;
    Ok(())
}
```

**Benefits:**
- Multiple readers can read simultaneously
- Writers don't block readers (WAL mode)
- All-or-nothing updates (transactions)

#### **Option 3: Optimistic Concurrency for Vector DB**

```rust
pub struct VectorDBWithVersioning {
    db: QdrantClient,
    version: Arc<AtomicU64>,
}

impl VectorDBWithVersioning {
    pub async fn update(&self, id: u64, vector: Vec<f32>) -> Result<()> {
        let current_version = self.version.load(Ordering::SeqCst);
        
        // Perform update
        self.db.upsert_point(id, vector).await?;
        
        // Increment version
        self.version.fetch_add(1, Ordering::SeqCst);
        
        Ok(())
    }
    
    pub async fn search_with_version(&self, query: &[f32]) -> (Vec<SearchResult>, u64) {
        let version = self.version.load(Ordering::SeqCst);
        let results = self.db.search(query).await;
        (results, version)
    }
}
```

---

## 8. Index Corruption & Data Loss

### Problem: What if Index Gets Corrupted?

**Causes:**
1. Disk full during write
2. Power loss during indexing
3. Process killed (OOM, Ctrl+C)
4. Filesystem errors
5. Software bugs

### Solutions

#### **Option 1: Checksums & Validation - MANDATORY**

```rust
pub struct IndexMetadata {
    version: String,
    checksum: String,
    last_update: i64,
}

impl Index {
    fn validate(&self) -> Result<bool> {
        // 1. Check metadata exists
        let metadata = self.load_metadata()?;
        
        // 2. Compute checksum of index files
        let computed = self.compute_checksum()?;
        
        // 3. Compare
        if metadata.checksum != computed {
            error!("Index corruption detected!");
            return Ok(false);
        }
        
        Ok(true)
    }
    
    fn save_with_checksum(&self) -> Result<()> {
        // 1. Write data
        self.write_data()?;
        
        // 2. Compute checksum
        let checksum = self.compute_checksum()?;
        
        // 3. Write metadata atomically
        let metadata = IndexMetadata {
            version: env!("CARGO_PKG_VERSION").to_string(),
            checksum,
            last_update: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        };
        
        // Atomic write (write to temp, then rename)
        let temp_path = self.path.join(".metadata.tmp");
        fs::write(&temp_path, serde_json::to_string(&metadata)?)?;
        fs::rename(&temp_path, self.path.join("metadata.json"))?;
        
        Ok(())
    }
}
```

#### **Option 2: Automatic Backup & Recovery**

```rust
pub struct IndexWithBackup {
    primary: PathBuf,
    backup: PathBuf,
}

impl IndexWithBackup {
    pub fn open(repo_path: &Path) -> Result<Self> {
        let primary = repo_path.join(".codesearch");
        let backup = repo_path.join(".codesearch.backup");
        
        // Try primary first
        if primary.exists() {
            if Index::validate(&primary).is_ok() {
                return Ok(Self { primary, backup });
            } else {
                warn!("Primary index corrupted, trying backup...");
            }
        }
        
        // Try backup
        if backup.exists() {
            if Index::validate(&backup).is_ok() {
                info!("Restored from backup");
                fs::rename(&backup, &primary)?;
                return Ok(Self { primary, backup });
            } else {
                error!("Both primary and backup corrupted!");
            }
        }
        
        // Start fresh
        info!("Creating new index");
        Ok(Self { primary, backup })
    }
    
    pub fn save(&self) -> Result<()> {
        // 1. Write to primary
        Index::save(&self.primary)?;
        
        // 2. Copy to backup (async, best-effort)
        let primary = self.primary.clone();
        let backup = self.backup.clone();
        tokio::spawn(async move {
            if let Err(e) = fs_extra::dir::copy(&primary, &backup, &CopyOptions::new()) {
                warn!("Backup failed: {}", e);
            }
        });
        
        Ok(())
    }
}
```

#### **Option 3: Rebuild Command**

```bash
$ codesearch rebuild

⚠️  This will delete and rebuild the entire index.
Continue? (y/N): y

Removing old index...
Parsing files...
[████████████████████] 100% (5,821 / 5,821 functions)
Generating embeddings...
[████████████████████] 100% (5,821 / 5,821 functions)

✓ Index rebuilt successfully
```

---

## 9. Migration & Breaking Changes

### Problem: Index Format Changes Between Versions

**Scenario:**
- v1.0.0: SQLite schema v1, vector format A
- v1.1.0: SQLite schema v2, vector format B
- User upgrades → **index is incompatible!**

### Solutions

#### **Option 1: Versioned Indexes - MANDATORY**

```rust
pub struct VersionedIndex {
    version: String,
    migration_path: Option<PathBuf>,
}

impl VersionedIndex {
    pub fn open(repo_path: &Path) -> Result<Self> {
        let metadata_path = repo_path.join(".codesearch/metadata.json");
        
        if !metadata_path.exists() {
            // New index
            return Ok(Self {
                version: env!("CARGO_PKG_VERSION").to_string(),
                migration_path: None,
            });
        }
        
        let metadata: IndexMetadata = serde_json::from_str(
            &fs::read_to_string(&metadata_path)?
        )?;
        
        // Check version compatibility
        match Self::check_compatibility(&metadata.version) {
            Compatibility::Compatible => Ok(Self { version: metadata.version, migration_path: None }),
            Compatibility::NeedsMigration => {
                info!("Index needs migration from {} to {}", metadata.version, env!("CARGO_PKG_VERSION"));
                Ok(Self { version: metadata.version, migration_path: Some(repo_path.to_path_buf()) })
            }
            Compatibility::Incompatible => {
                error!("Index version {} is incompatible. Please run: codesearch rebuild", metadata.version);
                Err(anyhow!("Incompatible index version"))
            }
        }
    }
    
    fn check_compatibility(old_version: &str) -> Compatibility {
        let old: Version = old_version.parse().unwrap();
        let new: Version = env!("CARGO_PKG_VERSION").parse().unwrap();
        
        // Breaking change: major version bump
        if old.major != new.major {
            return Compatibility::Incompatible;
        }
        
        // Migration needed: minor version bump
        if old.minor != new.minor {
            return Compatibility::NeedsMigration;
        }
        
        // Compatible: patch version
        Compatibility::Compatible
    }
}
```

#### **Option 2: Automatic Migration**

```rust
pub struct Migrator {
    migrations: Vec<Box<dyn Migration>>,
}

pub trait Migration {
    fn version(&self) -> String;
    fn migrate(&self, index_path: &Path) -> Result<()>;
}

struct MigrationV1toV2;

impl Migration for MigrationV1toV2 {
    fn version(&self) -> String {
        "1.0.0 -> 1.1.0".to_string()
    }
    
    fn migrate(&self, index_path: &Path) -> Result<()> {
        info!("Migrating index from v1.0.0 to v1.1.0...");
        
        // 1. Backup old index
        let backup = index_path.join("backup-v1.0.0");
        fs::rename(index_path, &backup)?;
        
        // 2. Create new index structure
        fs::create_dir(index_path)?;
        
        // 3. Copy data with transformation
        let old_db = Connection::open(backup.join("metadata.db"))?;
        let new_db = Connection::open(index_path.join("metadata.db"))?;
        
        // Run migration SQL
        new_db.execute_batch("
            -- New schema
            CREATE TABLE code_units_v2 (...);
            
            -- Copy data
            INSERT INTO code_units_v2
            SELECT ... FROM old_db.code_units;
        ")?;
        
        // 4. Update metadata
        let metadata = IndexMetadata {
            version: "1.1.0".to_string(),
            ...
        };
        fs::write(index_path.join("metadata.json"), serde_json::to_string(&metadata)?)?;
        
        info!("Migration complete!");
        Ok(())
    }
}
```

---

## 10. Network & API Rate Limits

### Problem: Cloud Embedding APIs Have Rate Limits

**Voyage API:**
- Rate limit: 1000 requests/minute
- 10,000 functions ÷ 64 batch size = 157 requests
- At limit: **~10 minutes** for initial index
- **Above limit: exponential backoff, errors**

**OpenAI API:**
- Rate limit: Depends on tier (60-3000 req/min)
- Cost: $0.0001 per 1k tokens
- 10,000 functions × 100 tokens avg = **$0.10**
- 100 repos = **$10** (acceptable)

### Solutions

#### **Option 1: Rate Limit Handling - MANDATORY**

```rust
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;

pub struct RateLimitedAPIClient {
    client: reqwest::Client,
    limiter: RateLimiter<
        governor::state::direct::NotKeyed,
        governor::state::InMemoryState,
        governor::clock::DefaultClock,
    >,
}

impl RateLimitedAPIClient {
    pub fn new(requests_per_minute: u32) -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(requests_per_minute).unwrap());
        let limiter = RateLimiter::direct(quota);
        
        Self {
            client: reqwest::Client::new(),
            limiter,
        }
    }
    
    pub async fn embed_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        // Wait for rate limiter
        self.limiter.until_ready().await;
        
        // Make request with retry
        let mut retries = 0;
        loop {
            match self.client.post("https://api.voyage.ai/embed")
                .json(&serde_json::json!({
                    "input": texts,
                    "model": "voyage-code-3"
                }))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    return Ok(resp.json().await?);
                }
                Ok(resp) if resp.status() == 429 => {
                    // Rate limit hit, exponential backoff
                    let wait_secs = 2u64.pow(retries);
                    warn!("Rate limited, waiting {}s", wait_secs);
                    tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                    retries += 1;
                    
                    if retries > 5 {
                        return Err(anyhow!("Rate limit exceeded after 5 retries"));
                    }
                }
                Err(e) => {
                    return Err(e.into());
                }
                _ => {
                    return Err(anyhow!("API error"));
                }
            }
        }
    }
}
```

#### **Option 2: Local Caching - OPTIMIZATION**

```rust
pub struct CachedEmbeddingClient {
    client: RateLimitedAPIClient,
    cache: Arc<Mutex<HashMap<String, Vec<f32>>>>,
    cache_path: PathBuf,
}

impl CachedEmbeddingClient {
    pub fn new(cache_path: PathBuf) -> Result<Self> {
        // Load cache from disk
        let cache = if cache_path.exists() {
            serde_json::from_str(&fs::read_to_string(&cache_path)?)?
        } else {
            HashMap::new()
        };
        
        Ok(Self {
            client: RateLimitedAPIClient::new(1000),
            cache: Arc::new(Mutex::new(cache)),
            cache_path,
        })
    }
    
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(embedding) = cache.get(text) {
                return Ok(embedding.clone());
            }
        }
        
        // Cache miss, call API
        let embedding = self.client.embed_batch(vec![text.to_string()]).await?
            .into_iter()
            .next()
            .unwrap();
        
        // Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(text.to_string(), embedding.clone());
        }
        
        // Persist cache periodically
        self.save_cache()?;
        
        Ok(embedding)
    }
}
```

**Benefits:**
- Deduplicates API calls
- Survives crashes (persisted to disk)
- Faster re-indexing

---

## 11. Security & Privacy Concerns

### Problem: Users Don't Want Code Sent to Cloud

**Concerns:**
1. Proprietary code sent to third-party APIs
2. API keys stolen from config files
3. Code exfiltration via network
4. Malicious model downloads

### Solutions

#### **Option 1: Opt-In Cloud Features - MANDATORY**

```bash
# Default: Local-only (no network)
$ codesearch index
Using local model (UniXcoder)
No data sent to internet ✓

# Explicit opt-in for cloud
$ codesearch config --model voyage-code-3 --api-key YOUR_KEY
⚠️  Warning: This will send code to Voyage AI servers.
Continue? (y/N): y

API key saved to ~/.config/codesearch/config.toml
```

#### **Option 2: API Key Security**

```rust
// Never store API keys in plaintext!

use keyring::Entry;

pub struct SecureConfig {
    entry: Entry,
}

impl SecureConfig {
    pub fn new() -> Result<Self> {
        Ok(Self {
            entry: Entry::new("codesearch", "api_key")?,
        })
    }
    
    pub fn save_api_key(&self, key: &str) -> Result<()> {
        // Store in OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service)
        self.entry.set_password(key)?;
        Ok(())
    }
    
    pub fn get_api_key(&self) -> Result<String> {
        self.entry.get_password().map_err(|e| anyhow!("API key not found: {}", e))
    }
}
```

**Benefits:**
- Keys not in plaintext config files
- Protected by OS security
- Encrypted at rest

#### **Option 3: Local-First by Default**

```rust
pub enum EmbeddingMode {
    LocalOnly,    // UniXcoder, GraphCodeBERT (default)
    CloudAPI,     // Voyage, OpenAI (opt-in)
}

impl Default for EmbeddingMode {
    fn default() -> Self {
        Self::LocalOnly
    }
}
```

---

## 12. Performance Degradation Over Time

### Problem: Index Gets Slower as It Grows

**Causes:**
1. SQLite fragmentation
2. Vector DB index degradation
3. Dead entries (deleted code)
4. Log files growing unbounded

### Solutions

#### **Option 1: Automatic Maintenance - MANDATORY**

```rust
pub struct IndexMaintenance {
    last_vacuum: SystemTime,
    last_optimize: SystemTime,
}

impl IndexMaintenance {
    pub fn run_if_needed(&mut self, db: &Connection) -> Result<()> {
        let now = SystemTime::now();
        
        // Vacuum every 7 days
        if now.duration_since(self.last_vacuum)? > Duration::from_secs(7 * 24 * 3600) {
            info!("Running VACUUM...");
            db.execute_batch("VACUUM;")?;
            self.last_vacuum = now;
        }
        
        // Optimize every 24 hours
        if now.duration_since(self.last_optimize)? > Duration::from_secs(24 * 3600) {
            info!("Running ANALYZE...");
            db.execute_batch("ANALYZE;")?;
            self.last_optimize = now;
        }
        
        Ok(())
    }
}
```

#### **Option 2: Cleanup Command**

```bash
$ codesearch cleanup

Analyzing index...
├─ Found 234 orphaned embeddings (deleted functions)
├─ Found 1,234 KB fragmented space in SQLite
├─ Found 2.3 MB old log files

Actions:
1. Delete orphaned embeddings
2. VACUUM SQLite database
3. Rotate log files

Estimated space reclaimed: 3.5 MB
Estimated time: 10 seconds

Continue? (Y/n): y

[████████████████████] 100%

✓ Cleanup complete
  - Deleted 234 orphaned entries
  - Reclaimed 3.5 MB disk space
  - Database optimized
```

---

## 13. Revised Architecture

### Final Production Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    CLI Interface                        │
│  (Beautiful TUI, Progress Bars, Error Handling)         │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────────┐
│                 Search Coordinator                       │
│  - Multi-backend fusion                                  │
│  - Result ranking & deduplication                        │
│  - Query optimization                                    │
└─────┬──────────┬──────────┬─────────────────────────────┘
      │          │          │
      │          │          │
┌─────▼────┐ ┌──▼──────┐ ┌─▼───────────────────────────┐
│   AST    │ │  BM25   │ │   Vector Search (Optional)   │
│ Backend  │ │ Backend │ │                              │
│          │ │         │ │  ┌────────────────────────┐  │
│ Tree-    │ │ Tantivy │ │  │  Strategy:             │  │
│ sitter   │ │ FTS     │ │  │  - IVF clustering      │  │
│          │ │         │ │  │  - PQ compression      │  │
│ Timeout  │ │         │ │  │  - Disk-backed (mmap)  │  │
│ 10s      │ │         │ │  │  - Hot cache (LRU)     │  │
│          │ │         │ │  └────────────────────────┘  │
└──────────┘ └─────────┘ └─────────────────────────────┘
      │          │          │
      └──────────┴──────────┘
                 │
┌────────────────▼────────────────────────────────────────┐
│               Storage Layer                              │
│                                                          │
│  ┌──────────────┐  ┌────────────────┐  ┌─────────────┐ │
│  │   SQLite     │  │ Qdrant/Custom  │  │  Tantivy    │ │
│  │   (WAL mode) │  │  Vector DB     │  │  Index      │ │
│  │              │  │                │  │             │ │
│  │  - Metadata  │  │  - Embeddings  │  │  - FTS      │ │
│  │  - Call graph│  │  - PQ codes    │  │  - Inverted │ │
│  │  - Checksums │  │  - Quantized   │  │    index    │ │
│  │              │  │  - Compressed  │  │             │ │
│  └──────────────┘  └────────────────┘  └─────────────┘ │
│                                                          │
│  Total per 10k functions: ~15-20 MB                      │
└──────────────────────────────────────────────────────────┘
                 │
┌────────────────▼────────────────────────────────────────┐
│            File System Layer                             │
│                                                          │
│  .codesearch/                                            │
│  ├── metadata.db          (SQLite, 5 MB)                │
│  ├── vectors.db           (Qdrant/Custom, 1-2 MB)       │
│  ├── tantivy/             (BM25 index, 8 MB)            │
│  ├── metadata.json        (version, checksum)           │
│  ├── index.lock           (file lock)                   │
│  └── .gitignore           (don't commit indexes)        │
│                                                          │
│  .codesearch.backup/      (auto backup)                 │
└──────────────────────────────────────────────────────────┘
                 │
┌────────────────▼────────────────────────────────────────┐
│          Incremental Indexing                            │
│                                                          │
│  ┌──────────────────────────────────────────────────┐  │
│  │  File Watcher (Smart Strategy Selection)        │  │
│  │  1. Git-based (watches .git/index) - BEST       │  │
│  │  2. Native (notify crate) - FALLBACK            │  │
│  │  3. Polling (5s interval) - LAST RESORT         │  │
│  └──────────────────────────────────────────────────┘  │
│                      │                                   │
│  ┌──────────────────▼──────────────────────────────┐  │
│  │  Change Queue (async processing)                │  │
│  │  - Debouncing (500ms)                           │  │
│  │  - Batching (process multiple files together)   │  │
│  │  - Priority queue (edited files first)          │  │
│  └──────────────────────────────────────────────────┘  │
│                      │                                   │
│  ┌──────────────────▼──────────────────────────────┐  │
│  │  Incremental Update Engine                      │  │
│  │  1. Parse file (Tree-sitter, timeout 10s)       │  │
│  │  2. Diff old vs new (find changed functions)    │  │
│  │  3. Update metadata (SQLite transaction)        │  │
│  │  4. Generate embeddings (only changed)          │  │
│  │  5. Update vector DB (upsert)                   │  │
│  │  6. Update BM25 index (Tantivy)                 │  │
│  └──────────────────────────────────────────────────┘  │
│                                                          │
│  Performance: ~0.5-3s per file change                    │
└──────────────────────────────────────────────────────────┘
```

---

## Production Checklist

### Before Launch

- [ ] **Scalability**
  - [ ] Test with 100,000+ function codebase
  - [ ] Test with 100+ repositories
  - [ ] Verify memory usage < 500 MB
  - [ ] Verify disk usage < 50 MB per 10k functions
  
- [ ] **Performance**
  - [ ] Search latency < 100ms (fast mode)
  - [ ] Search latency < 500ms (semantic mode)
  - [ ] Initial indexing < 10 min for 10k functions
  - [ ] Incremental update < 5s per file
  
- [ ] **Reliability**
  - [ ] Handle corrupted indexes gracefully
  - [ ] Automatic recovery from crashes
  - [ ] File watcher works on all platforms
  - [ ] No data loss on power failure
  
- [ ] **Compatibility**
  - [ ] Linux (x86_64, ARM64)
  - [ ] macOS (Intel, Apple Silicon)
  - [ ] Windows (x86_64)
  - [ ] Test on Ubuntu, Fedora, Arch, Debian
  
- [ ] **Security**
  - [ ] API keys in OS keychain
  - [ ] Opt-in for cloud features
  - [ ] No code sent to network by default
  
- [ ] **User Experience**
  - [ ] Progress bars for long operations
  - [ ] Clear error messages
  - [ ] Automatic migrations
  - [ ] Resume interrupted indexing
  - [ ] Graceful degradation (fallbacks)
  
- [ ] **Documentation**
  - [ ] Installation guide (all platforms)
  - [ ] Quick start
  - [ ] Troubleshooting (common issues)
  - [ ] Configuration reference
  - [ ] API documentation (if applicable)
  
- [ ] **Testing**
  - [ ] Unit tests (80%+ coverage)
  - [ ] Integration tests
  - [ ] Performance benchmarks
  - [ ] Stress tests (large codebases)
  - [ ] Cross-platform CI/CD

---

## Critical Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Vector DB** | IVF + PQ + Mmap | Scales to millions without RAM explosion |
| **File Watcher** | Git-based primary | Handles millions of files, no inotify limits |
| **Tree-sitter** | Async + timeout | Prevents UI blocking, handles large files |
| **Embedding** | Batched + lazy | 10x faster indexing, only embed what's searched |
| **Storage** | Quantized + compressed | 100x disk space reduction |
| **Concurrency** | File locks + WAL | Multiple readers, safe concurrent access |
| **Index Format** | Versioned + migrations | Smooth upgrades, no re-indexing |
| **API Rate Limits** | Rate limiter + cache | Prevents API errors, reduces costs |
| **Security** | Local-first + keychain | Privacy by default, secure API keys |
| **Maintenance** | Auto vacuum + cleanup cmd | Performance stays consistent over time |

---

## Conclusion

This tool **will work** at scale if we:

1. **Optimize vector storage aggressively** (IVF + PQ + compression)
2. **Use git-based file watching** (not filesystem watchers)
3. **Implement robust error handling** (timeouts, fallbacks, recovery)
4. **Test on real-world large codebases** (100k+ functions)
5. **Provide excellent UX** (progress, resumable, clear errors)

**The architecture is sound. The technology exists. We just need to implement it carefully.**

Next steps:
1. Build MVP with fast mode (AST + BM25)
2. Add semantic mode with optimizations
3. Stress test with massive repos
4. Iterate based on real user feedback

**This will be the definitive code search tool. Let's build it right.**
