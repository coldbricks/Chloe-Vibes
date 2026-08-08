# WAVE-002 — peak-feel

**Status:** PLAN  
**Created:** 2026-08-08  
**Repo:** C:\Users\coldb\Chloe-Vibes  
**Depends on:** WAVE-001 (feel-depth) — VERIFY; several WAVE-001 items shipped but prefire is dead in the motor path  
**MAX COMPUTE:** ON  

## Goal (one sentence)

Ship the hardest, most incredible haptic climax experience: motor-honest silence and punch contrast, devastating edge-and-deny arcs, on-beat prefire lead, Android dead-man safety, and session/preset honesty — dual platform, parity-locked.

## Product feel contract

| Pillar | What the body must feel |
|--------|-------------------------|
| **Punch** | Hard kicks briefly exceed body level (velocity overshoot survives ceiling); gate-leading booms keep dynamic height |
| **Contrast** | True motor rest between kicks and during deny — hard-snap silence, not slew-softened dips |
| **Arc** | Edge & Deny actually runs; tease → surge sequenced; post-deny return punch; maturity escalates over long sessions |
| **Honesty** | Prefire lands ~50 ms early on the motor path; UI labels match engine; dual-motor climax reaches Edge/Nora |

## Priority order (load-bearing)

1. Motor-honest silence / contrast  
2. Climax edge-deny excellence  
3. Latency / prefire  
4. Android safety watchdog  
5. Preset / session arc  
6. UX under arousal  

## Non-goals

- Full Phase-2 slew redesign (symmetric 15 ms + Classic Pump) without Domi hardware A/B — prep only if it unblocks silence honesty  
- FIND BOOM Android port  
- CI / release packaging / version bump  
- Broad UI redesign beyond arousal-critical safety + climax readability  
- Device firmware / Lovense protocol changes outside existing NUS command set  
- Optional tempo IOI median/mode clustering (nice)  
- Composite Response readout (Phase-0 honesty; nice if time)  

## Ownership

| Owner | Paths | Role |
|-------|-------|------|
| Grok | `src/audio.rs`, `src/gui.rs`, `src/presets.rs`, `src/settings.rs`, `src/auto_lock.rs`, `tests/*`, Android audio + BLE + UI safety, WAVE | builder + integrator |

## Frozen interfaces

- Signal chain order: **Spectral → Gate → Beat → Envelope → Climax → Output** (desktop must match Android; WAVE-001 left desktop Beat-before-Gate)  
- Lovense 0–20 protocol; dual-motor only for DeviceType id ∈ `{"P"}` (Edge) unless Nora DualBody mapping is explicitly parity-fixed  
- Parity golden columns / scenario list — regenerate when envelope/climax outputs change  
- Consent ceiling / volume / gain / min / max / climax / trim: FIND BOOM and auto-lock still must not write these  

---

## Must-ship actions (ordered)

### M1 — Prefire actually fires (latency / punch)

**Why:** WAVE-001 lead is theater: `prefire_ok` sets `is_onset=true` but strength gates (1.02 outer, 1.05 drive) reject every predictive frame while flux is low. Largest open musical-punch win.

**Impl notes:**
- Expose / use `recent_onset_strength` (or `prefire_velocity`) from BeatDetector  
- On prefire path only: inject synthetic strength floor ~1.12–1.20 **or** bypass flux 1.02 reject  
- **One-shot latch** per predicted beat (`last_prefire_ms` / consume prediction) — without latch, multi-frame UI re-check causes ghost punches then double with real onset  
- Hard-reset onset ring (timestamps, count, index) when tempo lock fully clears (`conf < 0.05`)  
- Desktop: reorder pipeline to **Gate → Beat → prefire** (match Android + frozen chain); use **current-frame** gate for prefire  
- Single onset threshold **1.05** shared by GUI pre-gate and drive (kill 1.02–1.05 dead band)

**Files:**  
`src/gui.rs` (~1472–1534, 1504–1515); `src/audio.rs` (BeatDetector prefire, `drive`/`trigger`, `decay_tempo_confidence`);  
`android/.../audio/AudioCaptureManager.kt` (~637–663); `android/.../audio/BeatDetector.kt`; `android/.../audio/EnvelopeProcessor.kt`

**Parity:** Dual unit tests — locked grid → `prefire_ok` → envelope trigger; silence → conf decay → `!prefire`; latch no double-fire in 50 ms window. Optional golden path if motor columns change.

---

### M2 — Motor-honest boom silence + velocity punch (contrast)

**Why:** Deep boom needs true rest between kicks; overshoot is double-scaled then clamped to 1.0; gate-edge always triggers velocity=1.0 and blocks real onset for `min_retrigger_ms=20`.

**Impl notes:**
1. **silence_event on pluck/boom true-rest** — Idle entry / near-zero release (not only Sustain micro-pauses). Hosts already wire silence_class; hard-snap must fire.  
2. **Stop double-scale-then-clip** — velocity may scale magnitude *or* raise attack_target overshoot, but `process()` must not flatten super-peaks into a 1.0 plateau; Auto-Lock binary ≤0.88 headroom becomes real punch.  
3. **Gate-edge + imminent onset merge** — if gate opens then real onset within retrigger window, upgrade to velocity overshoot instead of locking velocity=1.0 thump.  
4. **Unify pluck vs organ** — `continuous_hold` and `enter_post_decay` use the **same** centroid-adjusted sustain so near-boundary pads do not Idle-retrigger stutter.

**Files:**  
`src/audio.rs` (`EnvelopeProcessor::trigger`/`process`/`drive`/`enter_post_decay`);  
`android/.../audio/EnvelopeProcessor.kt`;  
`src/gui.rs` + `AudioCaptureManager.kt` silence_class paths (verify only)

**Parity:** Envelope unit tests + golden regen if peak shapes change; new host tests for gate-then-onset.

---

### M3 — Silence-class owns the full output path (contrast + climax)

**Why:** TEMPORAL_ARCHITECTURE: depth ≥30% for 60–100 ms must bypass slew fall + peak-hold. Climax residual boost and desktop `map_output` omitting `silence_event` re-inject energy into rests.

**Impl notes:**
- Propagate `silence_event` through Climax: if silence, force 0 and clear gated_boost contribution  
- Desktop `map_output` / host: `is_silent |= silence_event` (match Android `silenceClass`)  
- Deep deny / tease cliffs (depth ≥30% for first 60–100 ms of drop) raise silence-class  
- Android BLE peak-hold: shorten (~20–25 ms) **or** bypass on large downward steps / near-rest (WAVE-001 zero-bypass stays)

**Files:**  
`src/audio.rs` (ClimaxEngine + map_output call sites); `src/gui.rs` (~1593–1657);  
`android/.../audio/ClimaxEngine.kt`; `AudioCaptureManager.kt` (~714–724, 828–850);  
`android/.../device/BleDeviceManager.kt` (peak-hold)

**Parity:** Golden scenario — long sustain, no drum retriggers; assert envelope zeros + silence_event path. Tiny host assert for hard-snap.

---

### M4 — Edge & Deny product path + devastating arc (climax)

**Why:** Factory "Edge & Deny" ships `climax_enabled=false` — named path is a no-op. Tease×surge cancel end-of-cycle peaks. Post-deny boost dies under clamp-to-1.0. Deny never true-rests.

**Impl notes:**
1. **Enable climax** on Edge & Deny with real edge params (both catalogs)  
2. **Sequence tease then surge** (non-overlapping terminal arc) — Slow Tease / Break Me / Ride the Beat must peak, not cancel  
3. **Post-deny return punch survives 1.0** — temporary headroom / makeup on exit frame, or multiplicative boost before soft ceiling  
4. **Deep deny → near-zero silence-class** (align M3); residual `(1-deny_depth)` alone is not enough  
5. **Freeze `high_output_ms` while `deny_active`** — count felt output, not pre-deny raw; restore multi-second recovery at maturity  
6. **Wire `motor2_output`** to dual-motor devices when climax enabled (keep treble×motor1 when climax off)  
7. **Cap micro-pulse ≤5 Hz** (motor-expressible band); move budget into depth / slower chaos  
8. Fix multi-wrap `cycle_count` (advance by floor(cycles), not +1); extend maturity past 6 cycles (slow log growth)

**Files:**  
`src/presets.rs` (Edge & Deny); `android/.../audio/Presets.kt`;  
`src/audio.rs` ClimaxEngine (~1300–1510); `android/.../audio/ClimaxEngine.kt`;  
`src/gui.rs` (~1773–1794); `AudioCaptureManager.kt` (~732–749)

**Parity:** Deny activation + maturity + re-trigger gap tests; golden regen if outputs change; dual-motor columns must match **live** path (climax motor2 when on, not false CI green).

---

### M5 — Android audio-path dead-man + durable stop (safety)

**Why:** Desktop stops if pipeline heartbeat stale >2s. Android primary audio→BLE has no dead-man; hang holds last intensity. Stop races can re-arm motors. Disconnect without zero leaves toy vibrating.

**Impl notes:**
1. **Dead-man** mirror desktop 2s / companion 1.75s — heartbeat stamp each processing frame; stale → `stopMotors`  
2. **Lease-fence motor writes** under `outputLock` (or monotonic generation) so post-stop frames cannot write  
3. **`stopMotors` before every disconnect** + verified stop with retry / UI error  
4. **Fail-closed `processingLoop`** — on >100 exceptions: `running=false` + Application stopMotors + final zero  
5. Sticky / red **Stop all** affordance (pairs with M8 UX)

**Files:**  
`android/.../ChloeVibesApplication.kt`; `AudioCaptureManager.kt` (heartbeat + loop exit);  
`BleDeviceManager.kt` (`disconnect`, `stopMotors`, pendingCommand priority);  
`MainScreen.kt`; `MainActivity.kt`

**Parity:** Unit test patterned on `FunscriptVibrationSessionControllerTest.staleHeartbeatCannotExtendDeadman`. No Rust change required for dead-man itself.

---

### M6 — Anti-adaptation rests that complete (contrast on pads)

**Why:** Every onset retrigger clears `next_micro_pause_ms`; continuous/pad material never gets true-zero rests. Pause floor 60 ms often shorter than Domi coast-down.

**Impl notes:**
- On retrigger: abort **active** pause only; do **not** zero `next_micro_pause_ms` (or subtract elapsed and keep deadline)  
- Raise pause duration floor to **~90–120 ms**  
- Arm first rest sooner (e.g. 0.8–2.5 s) on Sustain entry; keep longer gaps for subsequent  
- Cap climax micro-pulse (see M4)

**Files:**  
`src/audio.rs` EnvelopeProcessor Sustain / trigger; `android/.../EnvelopeProcessor.kt`

**Parity:** Unit test window asserts for pause duration; schedule-survives-retrigger test.

---

### M7 — Default / boom preset honesty (session arc foundation)

**Why:** Club 125 sets slew 48 ms vs Bass Drum 42 ms (softer trough). BOOM macros write names absent from factory_presets → restart force-applies Bass Drum. Android macros lack `outputSlewMs`. Edge & Deny climax off is M4.

**Impl notes:**
1. Align Club 125 slew with Bass Drum / WAVE-001 (**42 ms**); re-check Deep 90 / Hard 140 trough intent  
2. Promote **Deep 90 / Club 125 / Hard 140** into `factory_presets`; rename Android Chloe Loose/Medium/Ultimate to match  
3. Add `outputSlewMs` (and knee/curve as first-class where missing) to Preset snapshots; `applyPreset` must set them  
4. Settings load fallbacks: missing keys → **Hybrid + LowPass** (Bass Drum), not Dynamic + Full  
5. Kotlin Preset defaults: `thresholdKnee=0.15f`, `dynamicCurve=1.2f`

**Files:**  
`src/presets.rs`; `src/settings.rs`; `src/gui.rs` (`apply_chloe_rhythm_profile`, startup migration);  
`android/.../audio/Presets.kt`; `AudioCaptureManager.applyPreset`

**Parity:** Catalog name + Bass Drum number lock; dual-client boom macro trough match.

---

### M8 — UX under arousal (Android critical + desktop boom honesty)

**Why:** Build Cycle shows "90000 s"; Stop buried in scroll; climax opaque; ENERGY GATE plot lies post-volume; Floor hover encourages killing rest.

**Impl notes (Android — must):**
- Fix Build Cycle to **true seconds** (display + entry + range 8–240 s, write `*1000`)  
- Sticky bottom safety rail: **Stop** + output % + Climax toggle  
- Collapse expert sections; promote Volume, Ceiling, Climax  
- Expose climax **CYCLE % + Reset** (`lastClimaxPhase` on ProcessingState)  
- `keepScreenOn` while capturing  
- Couple Floor ≤ Ceiling; larger LIVE/STOPPED + touch targets  

**Impl notes (Desktop — must for boom honesty):**
- Plot **gate-domain** energy on ENERGY GATE (not post-volume fill)  
- Clear `last_report` on auto_lock cancel; show report only when Locked  
- Cancel FIND BOOM on any listen-time preset/slider intervention  
- Rewrite Floor hover + soft warn when `min_vibe` lifts troughs  

**Files:**  
`android/.../ui/MainScreen.kt`; `MainActivity.kt`; `AudioCaptureManager.kt` / `ClimaxEngine.kt` (phase export);  
`src/gui.rs`; `src/auto_lock.rs`

**Parity:** Behavior parity for climax phase/reset; no golden impact for pure UI.

---

### M9 — BLE DeviceType + curve session integrity (Android feel foundation)

**Why:** Without DeviceType, Edge stays mono; stale rest/gamma across reconnect; peak-hold fills troughs.

**Impl notes:**
- Priority control queue + retry until `applyDeviceType` (DeviceType not last-write-wins vs Vibrate)  
- Reset `isDualMotor` + rest/gamma defaults in `connectInternal` and disconnect  
- Provisional curves from advertised LVS code / `modelHint` at connect; confirm on DeviceType  
- Align Nora (A/C) with DualBody if desktop maps nora→DualBody  

**Files:**  
`android/.../device/BleDeviceManager.kt`; `LovenseProtocol.kt`; optional `src/gui.rs` parity check for Nora

**Parity:** Cross-platform rest floor / dual path for same model class.

---

### M10 — Parity harness protects the feel contract

**Why:** Prefire, silence zeros, dual-motor live path, and post-map slew/hard-snap are under-tested; WAVE-001 "implemented" prefire without motor-path assertion.

**Impl notes:**
- Production-shaped golden wrapper: Gate → Beat → prefire → pre-gate → drive  
- Golden: long sustain → envelope zeros + silence_event  
- Dual-motor: golden covers **live** spectral motor2 **or** climax motor2 when climax on — pick one spec, document  
- Prefire → envelope trigger dual tests (Rust + Kotlin)  
- Deny activation + maturity tests (M4)

**Files:**  
`tests/parity.rs`; `tests/parity_golden.csv`; `android/.../ParityTest.kt`; unit tests in `src/audio.rs` + Kotlin twins

---

## Nice-to-have (if time after must-ship)

| ID | Item | Notes |
|----|------|-------|
| N1 | Attack honesty UI | Label attack_ms <50 as Instant; ADSR preview skips Attack; dim OVERRIDE A when inert |
| N2 | Surface Motor ramp | Desktop: one slider row under ENVELOPE / default-open OVERRIDE |
| N3 | Tempo IOI median/mode | Fold/cluster for eighth-note / swing prefire accuracy |
| N4 | Composite Response readout | Phase-0 display math; enables informed Classic Pump vs 15 ms A/B |
| N5 | Classic Pump flag prep | Setting + UI stub only — no Phase-2 flip without Domi A/B |
| N6 | Shared `apply_output_slew` | Extract to audio.rs + Kotlin for future Phase-2 one-commit |
| N7 | Android climax profile one-clicks | Edge/Overload/Punisher parity with desktop `apply_climax_profile` |
| N8 | Link-loss zero + post-reconnect rest | BleDeviceManager STATE_DISCONNECTED / Ready fence |
| N9 | UncaughtExceptionHandler + onStop zero | Best-effort panic/lifecycle stop for non-companion |
| N10 | Docs sync | TECHNICAL_REFERENCE: prefire 50 ms, slew 42 ms, micro-pauses 60–100 ms time-based, overshoot 1.28 |
| N11 | Soft late-cycle headroom | Beyond post-deny punch: soft ceiling so surge remains audible mid-plateau |
| N12 | Edge motor2 weaker rest/gamma | Optional unmatched-actuator curve |

---

## Deferred (explicit)

| Item | Why deferred |
|------|----------------|
| Phase-2 symmetric 15 ms slew + factory release retune | Needs Domi A/B; Classic Pump escape hatch; risk of killing signature pump |
| FIND BOOM Android port | WAVE-001 non-goal; desktop-only remains |
| Auto-Lock stop writing output_slew_ms | Couples to Phase-2 demotion |
| BLE minWriteInterval 20 ms A/B | Device-class experiment; not required for silence honesty |
| Wand continuous<0.5 hard-zero retune | Domi hang vs soft body tradeoff — hardware A/B |
| Full expert UI declutter (desktop OVERRIDE already partial) | Beyond arousal-critical set |
| Labeled-corpus integration / accelerometer Domi measure | Phase 4 of TEMPORAL_ARCHITECTURE |
| CI packaging / version bump | Separate release wave |

---

## Implementation notes — cross-cutting

### Dual-client lockstep
Any change to EnvelopeProcessor, ClimaxEngine, BeatDetector, or map/silence semantics lands **Rust + Kotlin in the same change set**. Prefer audio.rs as source of truth; port constants and branch order literally.

### Silence-class contract (binding)
An event is silence-class when:
- Envelope `silence_event` (micro-pause or boom/pluck true-rest), **or**
- Climax deep deny/tease cliff (depth ≥30% for first 60–100 ms), **or**
- True mapped zero  

Silence-class **must**: force output 0 before slew, bypass peak-hold, clear climax gated_boost for that frame.

### Prefire contract (binding)
- Lead: `PREFIRE_LEAD_MS = 50` (device-class compensation still open risk)  
- Predictive onset must reach `EnvelopeProcessor.drive` with strength ≥ drive threshold  
- At most one prefire per predicted beat; real onset may refine velocity inside retrigger window (M2 gate-merge) without double full-height punch  

### Climax edge-deny contract (binding)
- Named "Edge & Deny" preset: `climax_enabled=true` with non-default edge params  
- Deny creates near-rest (silence-class), not soft plateau  
- Post-deny overshoot feelable even when dry is already loud  
- Tease and surge do not multiplicatively cancel terminal peaks  
- Dual-motor climax uses engine `motor2_output` when climax on and device is dual  

---

## Acceptance commands

```text
cargo test
cargo test --test parity
cargo clippy -- -D warnings
cd android && ./gradlew testDebugUnitTest
```

**Hardware A/B (operator — not automated):**
1. Bass Drum / Club 125 on Domi: kick troughs reach true rest; punch on-beat (prefire)  
2. Edge & Deny on Edge: deny → stillness → return punch; motor2 spatial contrast  
3. Android: kill processing / force hang → motors stop ≤2s; Stop all verified zero  
4. Long session (~10+ min): deny recovery multi-second; maturity still escalates  

---

## Risks

| Risk | Mitigation |
|------|------------|
| Prefire without latch → double-hits | M1 one-shot latch mandatory in same commit as strength fix |
| 50 ms lead early on slow Android BLE | Keep constant; document; optional later device-class table |
| Post-deny headroom / soft ceiling changes golden peaks | Regen golden; document headroom policy |
| motor2 climax path changes Edge feel vs treble shadow | Feature only when climax enabled; treble path when off |
| Silence hard-snap feels "clicky" on some toys | Depth/duration gates; peak-hold bypass only near-rest / large down steps |
| Phase-2 not in wave but slew still musical (42 ms) | Accept; M3 hard-snap is the honesty path for rests |
| Android dead-man false-positive on UI pause | Heartbeat from processing thread only; mirror desktop 2s |
| Catalog rename breaks saved preset names | Migration: map Chloe Loose/Medium/Ultimate → Deep 90/Club 125/Hard 140 |
| Scope creep into full Phase-2 | Non-goal; N5/N6 prep only |

---

## Dispatch plan

1. **M1 + M10 prefire tests first** — largest latency win; prove motor-path consumption  
2. **M2 + M3 + M6** — silence/contrast stack (envelope + climax path + anti-adaptation)  
3. **M4 + M7** — climax product path + presets  
4. **M5 + M9** — Android safety + BLE integrity  
5. **M8** — UX under arousal (can parallelize with M5 on Android UI)  
6. Self-verify suite; hardware A/B by David before close  

## Ledger (live)

### Evidence
- (pending implementation)

### Implemented
- (none yet — plan only)

### Open risks
- Prefire BLE class compensation  
- Phase-2 Classic Pump still deferred  
- Dual-motor climax feel needs Edge hardware confirm  

### Close
- Observer: suite green + hardware A/B  
- Commit: (pending user)  
- Result: PLAN → implement on go-signal  
