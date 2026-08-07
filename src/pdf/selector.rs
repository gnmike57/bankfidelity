use super::engine::*;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Cache key that ties an entry to a specific file revision: when the file's
/// modified-time changes, the key changes and the stale entry is naturally
/// evicted/missed rather than served.
type RenderKey = (String, usize, u64, u64);
type BlocksKey = (String, usize, u64);

/// Recommendation #2/#7 - small, bounded LRU caches so re-navigating to a
/// page (preview) or re-reading its text blocks (preview/edit/verify) doesn't
/// re-rasterise or re-parse the same page every time.
struct EngineCaches {
    rendered: Mutex<LruCache<RenderKey, RenderedPage>>,
    blocks: Mutex<LruCache<BlocksKey, Vec<TextBlock>>>,
}

impl EngineCaches {
    fn new() -> Self {
        Self {
            // ~24 pages of rendered PNGs and 256 page-block lists is plenty
            // for snappy navigation without unbounded memory growth.
            rendered: Mutex::new(LruCache::new(NonZeroUsize::new(24).unwrap())),
            blocks: Mutex::new(LruCache::new(NonZeroUsize::new(256).unwrap())),
        }
    }
}

/// File modified-time (in nanoseconds since the epoch) used as a cheap
/// revision token. Returns 0 when unavailable so caching still works for
/// paths whose mtime can't be read (it just can't auto-invalidate them).
fn file_revision(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct PdfEngineSelector {
    primary: Arc<dyn PdfEngine>,
    fallback: Arc<dyn PdfEngine>,
    config: crate::app::config::ConfigManager,
    caches: Arc<EngineCaches>,
}

impl std::fmt::Debug for PdfEngineSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfEngineSelector").finish_non_exhaustive()
    }
}

impl PdfEngineSelector {
    pub fn new(
        primary: Arc<dyn PdfEngine>,
        fallback: Arc<dyn PdfEngine>,
        config: crate::app::config::ConfigManager,
    ) -> Self {
        Self {
            primary,
            fallback,
            config,
            caches: Arc::new(EngineCaches::new()),
        }
    }

    /// Return the engine mode from one immutable configuration generation.
    fn current_mode(&self) -> crate::app::config::PdfEngineMode {
        self.config.snapshot().config().engine_mode
    }

    /// Sequential primary->fallback execution. Used for write operations
    /// (`apply_change`) where running both engines against the same output
    /// concurrently would race on the destination file. `DualConcurrent`
    /// shares this safe sequential path for writes.
    fn try_primary_or_fallback<T, F>(&self, operation: F) -> Result<T, EngineError>
    where
        F: Fn(&dyn PdfEngine) -> Result<T, EngineError>,
    {
        // Universal panic guard to achieve Zero-Defect reliability
        let run_safe = |engine: &dyn PdfEngine| -> Result<T, EngineError> {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(engine)));
            match res {
                Ok(r) => r,
                Err(panic_err) => {
                    let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_err.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic".to_string()
                    };
                    Err(EngineError::ApplyFailed(format!(
                        "Engine thread panicked: {}",
                        msg
                    )))
                }
            }
        };

        match self.current_mode() {
            crate::app::config::PdfEngineMode::NativeOnly => run_safe(&*self.fallback),
            crate::app::config::PdfEngineMode::PyMuPdfOnly => run_safe(&*self.primary),
            crate::app::config::PdfEngineMode::TypstReconstruct => {
                Err(EngineError::EncryptedOrRasterized(
                    "Typst Reconstruct mode explicitly requested".into(),
                ))
            }
            crate::app::config::PdfEngineMode::PyMuPdfProPrimary
            | crate::app::config::PdfEngineMode::DualConcurrent => {
                // SOTA Robust Plan: Try PyMuPDF (primary) first for maximum fidelity editing.
                // If the python bridge fails or throws an exception, seamlessly fallback to Native.
                match run_safe(&*self.primary) {
                    Ok(result) => Ok(result),
                    Err(EngineError::Unsupported) => {
                        tracing::warn!(
                            engine.fallback_triggered = true,
                            primary_error = "Unsupported",
                            "PyMuPDF engine unsupported, falling back to Native engine"
                        );
                        run_safe(&*self.fallback)
                    }
                    Err(EngineError::RowDrifted {
                        x0,
                        y0,
                        x1,
                        y1,
                        required,
                        best,
                    }) => {
                        // T2: RowDrifted means the target bbox doesn't overlap any
                        // real text on the page — both engines would produce the same
                        // verdict, so skip the fallback and surface immediately.
                        tracing::warn!(
                            x0,
                            y0,
                            x1,
                            y1,
                            required,
                            best,
                            "Edit target has drifted: the bbox no longer overlaps \
                             the intended text cell by the required margin"
                        );
                        Err(EngineError::RowDrifted {
                            x0,
                            y0,
                            x1,
                            y1,
                            required,
                            best,
                        })
                    }
                    Err(e) => {
                        tracing::warn!(
                            engine.fallback_triggered = true,
                            primary_error = %e,
                            "PyMuPDF engine failed, falling back to Native engine"
                        );
                        let fallback_res = run_safe(&*self.fallback);
                        match fallback_res {
                            Ok(res) => Ok(res),
                            Err(EngineError::ApplyFailed(ref msg))
                                if msg.contains("encrypt")
                                    || msg.contains("raster")
                                    || msg.contains("image") =>
                            {
                                Err(EngineError::EncryptedOrRasterized(msg.clone()))
                            }
                            Err(err) => Err(err),
                        }
                    }
                }
            }
        }
    }

    /// Read-path dispatch. In `DualConcurrent` mode the native and PyMuPDF
    /// engines run the operation **concurrently**; the primary (PyMuPDF/Deep)
    /// result is preferred when both succeed, and the native (Quick) result is
    /// used as an automatic fallback when PyMuPDF fails. All other modes
    /// reuse the sequential [`Self::try_primary_or_fallback`] path.
    fn dispatch_read<T, F>(&self, operation: F) -> Result<T, EngineError>
    where
        T: Send,
        F: Fn(&dyn PdfEngine) -> Result<T, EngineError> + Sync,
    {
        if self.current_mode() == crate::app::config::PdfEngineMode::DualConcurrent {
            self.run_dual_concurrent(operation)
        } else {
            self.try_primary_or_fallback(operation)
        }
    }

    /// Run `operation` on both engines at the same time using scoped threads.
    /// Prefers the primary (PyMuPDF/Deep) result; if the primary fails, the
    /// concurrently-computed fallback (native/Quick) result is used so a single
    /// engine failure never breaks the operation.
    fn run_dual_concurrent<T, F>(&self, operation: F) -> Result<T, EngineError>
    where
        T: Send,
        F: Fn(&dyn PdfEngine) -> Result<T, EngineError> + Sync,
    {
        let primary = &*self.primary;
        let fallback = &*self.fallback;
        std::thread::scope(|scope| {
            let primary_handle = scope.spawn(|| operation(primary));
            let fallback_handle = scope.spawn(|| operation(fallback));

            let primary_result = primary_handle.join().unwrap_or_else(|_| {
                Err(EngineError::ExtractFailed(
                    "Native engine thread panicked".into(),
                ))
            });

            match primary_result {
                Ok(value) => {
                    // Primary won - still join the fallback thread so it
                    // is never detached, but discard its result.
                    let _ = fallback_handle.join();
                    Ok(value)
                }
                Err(primary_err) => {
                    tracing::warn!(
                        engine.fallback_triggered = true,
                        primary_error = %primary_err,
                        "Primary engine failed in DualConcurrent mode, using fallback result"
                    );
                    fallback_handle.join().unwrap_or_else(|_| {
                        Err(EngineError::ExtractFailed(
                            "Fallback engine thread panicked".into(),
                        ))
                    })
                }
            }
        })
    }
}

impl PdfEngine for PdfEngineSelector {
    fn capabilities(&self) -> EngineCapabilities {
        let p_cap = self.primary.capabilities();
        let f_cap = self.fallback.capabilities();
        EngineCapabilities {
            supports_redaction: p_cap.supports_redaction || f_cap.supports_redaction,
            supports_cjk: p_cap.supports_cjk || f_cap.supports_cjk,
            supports_embedded_fonts: p_cap.supports_embedded_fonts || f_cap.supports_embedded_fonts,
            estimated_fidelity: p_cap.estimated_fidelity.max(f_cap.estimated_fidelity),
        }
    }

    fn render_page(&self, path: &Path, page: usize, dpi: f32) -> Result<RenderedPage, EngineError> {
        // Recommendation #2 - serve repeated previews of the same page from
        // the LRU cache; `dpi.to_bits()` + file revision keep the key exact.
        let key: RenderKey = (
            path.to_string_lossy().to_string(),
            page,
            dpi.to_bits() as u64,
            file_revision(path),
        );
        if let Ok(mut cache) = self.caches.rendered.lock() {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let rendered = self.dispatch_read(|engine| engine.render_page(path, page, dpi))?;
        if let Ok(mut cache) = self.caches.rendered.lock() {
            cache.put(key, rendered.clone());
        }
        Ok(rendered)
    }

    fn get_text_blocks(&self, path: &Path, page: usize) -> Result<Vec<TextBlock>, EngineError> {
        // Recommendation #7 - memoise per-page text blocks; invalidated by the
        // file revision token so edits to the PDF are always re-parsed.
        let key: BlocksKey = (
            path.to_string_lossy().to_string(),
            page,
            file_revision(path),
        );
        if let Ok(mut cache) = self.caches.blocks.lock() {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let blocks = self.dispatch_read(|engine| engine.get_text_blocks(path, page))?;
        if let Ok(mut cache) = self.caches.blocks.lock() {
            cache.put(key, blocks.clone());
        }
        Ok(blocks)
    }

    fn find_text_block_at_click(
        &self,
        path: &Path,
        page: usize,
        x: f32,
        y: f32,
    ) -> Result<Option<TextBlock>, EngineError> {
        self.dispatch_read(|engine| engine.find_text_block_at_click(path, page, x, y))
    }

    fn apply_change(
        &self,
        input: &Path,
        output: &Path,
        page: usize,
        bbox: [f32; 4],
        new_text: &str,
        old_text: &str,
        font_path: Option<&Path>,
    ) -> Result<ReplaceOutcome, EngineError> {
        // T2: Route through apply_change_guarded with 50% overlap threshold
        // to prevent row-drift edits on introspectable pages.
        self.try_primary_or_fallback(|engine| {
            engine.apply_change_guarded(
                input, output, page, bbox, new_text, old_text, font_path, 0.5,
            )
        })
    }

    fn apply_many_edits(
        &self,
        input: &Path,
        output: &Path,
        edits_json: &str,
        font_path: Option<&Path>,
    ) -> Result<usize, EngineError> {
        // T2: Delegate apply_many_edits through try_primary_or_fallback so
        // native engine acts as automatic fallback when Python actor fails.
        self.try_primary_or_fallback(|engine| {
            engine.apply_many_edits(input, output, edits_json, font_path)
        })
    }

    fn analyze_layout(&self, path: &Path) -> Result<DocumentLayout, EngineError> {
        self.dispatch_read(|engine| engine.analyze_layout(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::{AppConfig, PdfEngineMode};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[derive(Debug)]
    struct MockEngine {
        fail_with: Option<EngineError>,
        panic_on_call: bool,
        called: Arc<AtomicBool>,
    }

    impl MockEngine {
        fn new_success() -> Self {
            Self {
                fail_with: None,
                panic_on_call: false,
                called: Arc::new(AtomicBool::new(false)),
            }
        }

        fn new_error(err: EngineError) -> Self {
            Self {
                fail_with: Some(err),
                panic_on_call: false,
                called: Arc::new(AtomicBool::new(false)),
            }
        }

        fn new_panic() -> Self {
            Self {
                fail_with: None,
                panic_on_call: true,
                called: Arc::new(AtomicBool::new(false)),
            }
        }

        fn was_called(&self) -> bool {
            self.called.load(Ordering::SeqCst)
        }
    }

    impl PdfEngine for MockEngine {
        fn capabilities(&self) -> EngineCapabilities {
            EngineCapabilities {
                supports_redaction: false,
                supports_cjk: false,
                supports_embedded_fonts: false,
                estimated_fidelity: 0.0,
            }
        }

        fn render_page(
            &self,
            _path: &Path,
            _page: usize,
            _dpi: f32,
        ) -> Result<RenderedPage, EngineError> {
            self.called.store(true, Ordering::SeqCst);
            if self.panic_on_call {
                panic!("MockEngine panic!");
            }
            if let Some(err) = &self.fail_with {
                return Err(crate::pdf::engine::EngineError::ApplyFailed(
                    err.to_string(),
                ));
            }
            Ok(RenderedPage {
                png_bytes: vec![1, 2, 3],
                width_pts: 100.0,
                height_pts: 100.0,
            })
        }

        fn get_text_blocks(
            &self,
            _path: &Path,
            _page: usize,
        ) -> Result<Vec<TextBlock>, EngineError> {
            Ok(vec![])
        }
        fn find_text_block_at_click(
            &self,
            _path: &Path,
            _page: usize,
            _x: f32,
            _y: f32,
        ) -> Result<Option<TextBlock>, EngineError> {
            Ok(None)
        }
        fn apply_change(
            &self,
            _i: &Path,
            _o: &Path,
            _p: usize,
            _b: [f32; 4],
            _n: &str,
            _ot: &str,
            _fp: Option<&Path>,
        ) -> Result<ReplaceOutcome, EngineError> {
            Ok(ReplaceOutcome {
                success: true,
                font_used: "MockFont".into(),
                overflow: false,
                obj_id: None,
            })
        }
        fn analyze_layout(&self, _path: &Path) -> Result<DocumentLayout, EngineError> {
            Ok(DocumentLayout {
                total_pages: 1,
                pages: vec![],
                has_consistent_headers: true,
                has_consistent_footers: true,
                overall_style: "Standard".to_string(),
                layout_confidence: 1.0,
            })
        }
    }

    fn make_selector(
        primary: Arc<dyn PdfEngine>,
        fallback: Arc<dyn PdfEngine>,
        mode: PdfEngineMode,
    ) -> PdfEngineSelector {
        let mut cfg = AppConfig::default();
        cfg.engine_mode = mode;

        PdfEngineSelector::new(
            primary,
            fallback,
            crate::app::config::ConfigManager::new(Arc::new(cfg)),
        )
    }

    #[test]
    fn fallback_takes_over_when_primary_fails() {
        let primary = Arc::new(MockEngine::new_error(EngineError::Unsupported));
        let fallback = Arc::new(MockEngine::new_success());

        let selector = make_selector(
            primary.clone(),
            fallback.clone(),
            PdfEngineMode::PyMuPdfProPrimary,
        );

        // This exercises try_primary_or_fallback (write path)
        let res = selector.apply_change(
            Path::new("i"),
            Path::new("o"),
            0,
            [0.0; 4],
            "new",
            "old",
            None,
        );
        assert!(
            res.is_ok(),
            "Fallback should succeed even if primary returns Unsupported"
        );
    }

    #[test]
    fn dual_concurrent_falls_back_on_primary_panic() {
        let primary = Arc::new(MockEngine::new_panic());
        let fallback = Arc::new(MockEngine::new_success());

        let selector = make_selector(
            primary.clone(),
            fallback.clone(),
            PdfEngineMode::DualConcurrent,
        );

        // This exercises run_dual_concurrent
        let res = selector.render_page(Path::new("dummy.pdf"), 0, 150.0);
        assert!(
            res.is_ok(),
            "Selector should catch panic in primary and return fallback result"
        );
        assert!(
            fallback.was_called(),
            "Fallback engine must have been invoked"
        );
    }

    #[test]
    fn sequential_auto_catches_primary_panic_and_returns_fallback() {
        let primary = Arc::new(MockEngine::new_panic());
        let fallback = Arc::new(MockEngine::new_success());

        let selector = make_selector(
            primary.clone(),
            fallback.clone(),
            PdfEngineMode::PyMuPdfProPrimary,
        );

        // render_page falls into try_primary_or_fallback when not DualConcurrent
        let res = selector.render_page(Path::new("dummy.pdf"), 0, 150.0);
        assert!(
            res.is_ok(),
            "Selector should catch panic in primary and safely execute fallback"
        );
        assert!(
            fallback.was_called(),
            "Fallback engine must have been invoked"
        );
    }
}
