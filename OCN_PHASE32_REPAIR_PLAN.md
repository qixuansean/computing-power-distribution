# OCN Phase 3.2 — Repair Plan (Mind / Policy / TTM)

**Date:** 2026-11-23  
**Status:** Plan only. **No code changed. No Git operation performed.**  
**Basis:** `OCN_EXTENDED_AUDIT.md` findings F-17 … F-22.

---

## 1. Defect Summary

| ID | Defect | Evidence | Status |
|---|---|---|---|
| C-1 | `PolicyEngine.max_cpu_usage` / `max_memory_mb` never read | `scheduler_bridge.rs:5-18`, `:20-75`; compiler warning "never read" | `FAILED` |
| C-2 | TTM `cpu_usage` not clamped in Rust | `granite_ttm.rs:122`; clamp only at `runtime_service.py:76-77` | `PARTIAL` |
| C-3 | `memory_mb` and `confidence` hard-coded | `granite_ttm.rs:132,134,137` | `PARTIAL` |
| C-4 | No fallback provider on timeout/error | `scheduler_bridge.rs:118-121` returns `Err(Timeout)` | `MISSING` |
| C-5 | "TTM inference" is a heuristic simulation | `runtime_service.py:64-72`; `test_inference.py:45,60-70`; `rust_adapter.rs:43-72` | `FAILED` (claim) |
| C-6 | `DataSource` provenance type uncompiled | `schema.rs:6-11`, `baselines/rule_based.rs:22`, absent from `lib.rs` | `MISSING` |
| C-7 | Orphan modules (`runtime`, `schema`, `benchmark`, `baselines`) | `lib.rs:1-8` declares only `interfaces`, `providers`, `scheduler_bridge` | `PARTIAL` |
| C-8 | 512-point history contract unvalidated on Rust side | `runtime_service.py:53-54`; Rust default `vec![0.3; 512]` at `granite_ttm.rs:112-114` | `PARTIAL` |

---

## 2. Repair Work Packages

### R1 — Make `PolicyEngine` limits effective

**Root cause (exact):** the constructor stores `max_cpu_usage` and `max_memory_mb` (`scheduler_bridge.rs:12-18`), but `validate_recommendation` compares only against `request.budget.max_cpu` (`:43`) and `request.budget.max_memory_mb` (`:48`). The two fields are therefore dead; the compiler confirms it.

**Design:**
- Effective ceiling = `min(engine_limit, request_budget_limit)` for both CPU and memory.
- Apply to `LocalExecute` (must fit both), and to `RemoteOverflow` (the *origin* node's budget must be insufficient, otherwise local execution should have been chosen).
- Keep `min_confidence` behavior unchanged.
- Add `PolicyEngineError` variants? No — reuse `MindError::ValidationFailed` with explicit messages (public interface unchanged).

**Acceptance tests:**
| Test | Setup | Expected |
|---|---|---|
| P-1 | engine 0.8, budget 1.0, estimate 0.9 | `Err` |
| P-2 | engine 0.8, budget 0.5, estimate 0.4 | `Ok` |
| P-3 | engine 2048 MB, budget 4096 MB, estimate 3000 MB | `Err` |
| P-4 | engine 0.8, budget 0.8, estimate 0.8 (boundary) | `Ok` |

**Risk:** callers that relied on a permissive budget will start seeing rejections. Mitigation: log the effective ceiling; ship as a documented behavior change.

---

### R2 — Clamp and validate TTM outputs on both sides

**Design (Rust, `granite_ttm.rs`):**
```
if py_resp.prediction.len() != 96 -> Err(MindError::InvalidOutput)
avg = mean(prediction)
if !avg.is_finite() -> Err(MindError::InvalidOutput)
cpu_usage = avg.clamp(0.0, 1.0)
memory_mb  = derived_from_task_type(...).clamp(128, 8192)
confidence = 0.88f32.clamp(0.0, 1.0)   // replace literal with calibrated value later
```
- Keep the Python clamp (`runtime_service.py:76-77`) as defense in depth.
- Add `metadata["clamped"] = "true"` when a clamp actually changed a value, for observability.

**Acceptance tests:**
| Test | Input | Expected |
|---|---|---|
| T-1 | prediction contains 1.5 | `cpu_usage == 1.0`, `clamped=true` |
| T-2 | prediction contains -0.2 | `cpu_usage == 0.0` |
| T-3 | prediction length 95 | `Err(InvalidOutput)` |
| T-4 | prediction contains NaN | `Err(InvalidOutput)` |
| T-5 | normal range | unchanged, `clamped` absent |

**Risk:** silent saturation hides model drift. Mitigation: emit a counter/metric when clamped.

---

### R3 — Fallback provider and circuit breaker

**Design:** a decorator, not a change to `MindProvider`:

```rust
// ocn-mind/src/resilience.rs (new module)
pub struct FailoverProvider {
    primary: Arc<dyn MindProvider>,
    fallback: Arc<dyn MindProvider>,
    breaker: Arc<Mutex<BreakerState>>,   // failures, opened_at, half_open
    primary_timeout_ms: u64,
}
```
Behavior:
1. Call primary with `tokio::time::timeout(primary_timeout_ms)`.
2. On `Ok(Ok(_))` → return, reset breaker.
3. On `Ok(Err(_))` or timeout → increment failure count; call fallback; return fallback result with metadata:
   - `fallback_from = primary.provider_name()`
   - `fallback_reason = "timeout" | "error: ..."`
   - `fallback_latency_us`
4. After `N = 3` consecutive failures within 60 s → open circuit for 30 s: skip primary entirely, use fallback.
5. After cooldown → half-open: allow one primary probe.

**Latency honesty:**
- Detection of a timeout costs the configured timeout (default `ProviderConfig.timeout_ms = 5000`).
- The *switch* to the in-process rule-based provider after detection is sub-millisecond.
- The contract's "`<1ms` fallback" (`OCN_MIND_INTEGRATION_CONTRACT.md:29,43`) must be reworded to "**sub-millisecond switch after detection; detection bounded by `timeout_ms`**". Do not claim end-to-end `<1ms`.

**Acceptance tests:**
| Test | Setup | Expected |
|---|---|---|
| F-1 | primary sleeps 100 ms, timeout 10 ms | fallback result, `fallback_reason=timeout` |
| F-2 | primary returns `Err` | fallback result, `fallback_reason=error` |
| F-3 | 3 failures then 1 call | primary skipped, `fallback_reason=circuit_open` |
| F-4 | after cooldown | one primary probe attempted |
| F-5 | primary healthy | no fallback metadata |

**Risk:** fallback masks a persistent primary outage. Mitigation: breaker state + counters exported; alert after 10 consecutive opens.

---

### R4 — Truthful data provenance

**Problem:** The current TTM path produces a **heuristic simulation** (`runtime_service.py:64-72`) but is reported as TTM model inference. `DataSource` exists only in uncompiled code.

**Design (non-breaking first step):**
- Add a required metadata key on every `TaskAnalysisResult`:
  - `data_source` = `SIMULATED | REAL_DEVICE | HISTORICAL | SYNTHETIC`
  - `inference_mode` = `simulated_heuristic | real_forward_pass | rule_based`
  - `model_id` = `granite-timeseries-ttm-r2` (only when weights are loaded)
- Current `GraniteTTMProvider` must emit `data_source=SIMULATED`, `inference_mode=simulated_heuristic` until R5 lands.
- `RuleBasedProvider` emits `data_source=SIMULATED`, `inference_mode=rule_based`.

**Second step (interface change, requires approval):** add `pub data_source: DataSource` to `TaskAnalysisResult`. This touches the public interface; do not do it without explicit sign-off.

**Documentation policy:**
- Any report citing TTM results must state `inference_mode=simulated_heuristic`.
- Device performance may only be reported with `data_source=REAL_DEVICE` collected on-device.
- Existing documents (`OCN_TTM_REAL_MODEL_REPORT.md`, `OCN_TTM_FORECAST_VALIDATION.md`) should carry a correction note. **Not edited in this session** (no overwrite rule).

---

### R5 — Real inference path (decision deferred)

Options:

| Option | Description | Effort | Verdict |
|---|---|---|---|
| a | Implement the TTM forward pass in Python (patch embedding + mixer blocks + head) using the loaded safetensors | 3–5 days | Correct but heavier |
| b | Export to ONNX/TorchScript and run in Rust via `ort` | 2–3 days + build complexity | Good long-term |
| c | Keep the heuristic and rename the provider to `HeuristicResourceProvider` | 0.5 day | Honest short-term |

**Recommendation:** adopt (c) immediately for honesty, then (a). Do not call the heuristic "TTM inference" in the meantime.

---

### R6 — Orphan module disposition

| File | Current state | Options |
|---|---|---|
| `src/runtime.rs` | Second `MindProvider` trait + `MindError` | Rename trait to `LegacyMindProvider` and wire, or archive |
| `src/schema.rs` | `DataSource`, `NodeSnapshot`, `SchedulingRecord` | Wire `pub mod schema;` (no name conflicts) |
| `src/benchmark.rs` | Uses legacy runtime trait | Wire after R6.1 |
| `src/baselines/rule_based.rs` | Example of `DataSource::SIMULATED` | Wire after R6.1 |

**No files moved or deleted.** Plan: either declare modules (after renaming the duplicate trait) or leave as reference and document.

---

### R7 — 512-point history contract

- Rust: validate `history.len() == 512`; if not, substitute `vec![0.3; 512]` and set `metadata["history_substituted"]="true"`.
- Reject NaN/Inf in the history.
- Python already errors on wrong length (`runtime_service.py:53-54`); the Rust guard prevents a round-trip failure.

**Acceptance tests:** length 0/511/512/513; NaN element.

---

## 3. Ordering

```
R2 ──┐
R7 ──┼──> R1 ──> R3 ──> R4 ──> R5 (decision) ──> R6
     │
     └── independent
```

- R2 and R7 are local and low-risk; do first.
- R1 changes acceptance behavior; announce before shipping.
- R3 depends on R1 (fallback must also pass the stricter policy).
- R4 is documentation/metadata only.
- R6 last (touches module graph).

---

## 4. Test Matrix (Phase 3.2)

| ID | Scope | Pass criterion |
|---|---|---|
| M-01 | Existing 7 mind tests | Still pass |
| M-02 | P-1 … P-4 | Effective ceiling enforced |
| M-03 | T-1 … T-5 | Clamp and reject correctly |
| M-04 | F-1 … F-5 | Fallback and breaker correct |
| M-05 | metadata audit | Every result has `data_source` + `inference_mode` |
| M-06 | history length cases | Substitute + flag, no crash |
| M-07 | No NaN/Inf reaches PolicyEngine | Rejected upstream |

**Recovery:** if M-02 changes break existing callers, revert R1 only; R2/R7 remain valid.

---

## 5. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Stricter policy rejects previously valid local work | Medium | Announce; log effective ceilings; allow per-task override only above the engine floor |
| Fallback hides model failure | High | Breaker metrics + alerting; never mark fallback results as primary |
| Clamping masks drift | Medium | `clamped` counter |
| Interface change for `data_source` breaks callers | Medium | Use metadata first; field change in v2 |
| Breaker flapping under intermittent latency | Low | Half-open with single probe; cooldown 30 s |
| Rewriting the contract's `<1ms` claim causes confusion | Low | Correction note in new documents only |

---

## 6. Explicit Non-Claims

- The current provider does **not** perform TTM neural inference; it is a trend/sine heuristic (`runtime_service.py:64-72`).
- The 2.3 s full-chain test measures subprocess and `torch` import time, **not** forecast quality or device performance.
- No device-side Mind/Policy run exists.
- The `<1ms` fallback statement in the old contract is inaccurate as end-to-end language.

---

## 7. Approval Gates

1. Approve C-1 … C-8 as valid defects.
2. Approve R1 behavior change (effective ceiling).
3. Approve R3 fallback semantics and breaker thresholds (3 failures / 60 s / 30 s cooldown).
4. Choose R5 option (recommended: (c) now, (a) later).
5. Approve or defer the `TaskAnalysisResult.data_source` interface field (metadata-only is non-breaking).

No code has been written. No file has been moved or deleted.
