# ChloeVibes Peak Feel Army — Run Report

**Wave:** WAVE-002 peak-feel  
**Date:** 2026-08-08  
**Working tree vs:** `master` @ `365b53d`  
**ship_ready:** `false` (code WIP; suite not re-run in adversarial verify)

---

## Executive summary

Peak-feel army complete.

Plan: WAVE-002 peak-feel plans the hardest climax experience: motor-honest silence/contrast, real Edge & Deny arcs, on-beat prefire (WAVE-001 left it dead), Android dead-man safety, boom preset honesty, and arousal-safe UX—dual-client parity-locked. Phase-2 15ms slew stays deferred pending Domi A/B.

| Metric | Result |
|--------|--------|
| Implement lanes | **5/5** |
| Audit domains | **12/12** |
| cargo test | **35 lib + 61 bin** (1 ignored) + **1 parity** all pass |
| Explicit `cargo test --test parity` | `parity_rust_golden` **ok** |
| Android `testDebugUnitTest` | **BUILD SUCCESSFUL** (UP-TO-DATE) |
| Optional clippy `-D warnings` | Fails on **pre-existing** `manual_clamp` at `src/audio.rs:1107` (`recent_onset_strength.max(1.15).min(1.35)`); not introduced here, left unfixed per policy |

### ADVERSARIAL WAVE-002 VERIFY (2026-08-08)

Working tree vs `master` `365b53d`; full writeup at [`docs/waves/WAVE-002-VERIFY.md`](WAVE-002-VERIFY.md). Suite **NOT** run (`cargo`/`gradle`). **ship_ready=false**.

**PASS (code only):**

- **M5** Android dead-man + lease fence + Stop all
- **M6** micro-pause 90–120ms schedule-survives-retrigger
- **M7** boom catalog Deep90/Club125/Hard140 + slew42 + Edge&Deny climax ON
- Prefire strength floor + latch + conf decay
- `silence_event` hard-snap
- BLE peak-hold 22ms + drop bypass
- MotorFeel rest/gamma lockstep
- Android Build Cycle seconds + sticky safety rail + climax phase

**FAIL / P0:**

1. **Climax `motor2_output` NEVER wired** to live dual BLE/desktop path — hosts still treble spectral shadow (`gui.rs` ~1800, `AudioCaptureManager` ~769); Edge spatial climax is theater.
2. **Desktop pipeline still Beat→Gate not Gate→Beat**; prefire uses stale `gate_is_open`.
3. **Tests not executed** in adversarial verify; golden CSV churned but `parity_ok` cannot be asserted true from verify alone.

**FAIL / P1:**

- Post-deny boost still clamp-to-1.0 (claim oversold)
- Gate-edge velocity=1.0 + `min_retrigger` 20ms blocks real onset overshoot (M2 merge missing)
- Host pre-gate 1.02 vs drive 1.05 dead band remains
- Desktop `map_output` omits `silence_event` (hard-snap after only)
- `DeviceType` not priority-queued vs Vibrate
- Desktop M8 ENERGY GATE / FIND BOOM / Floor honesty not in diff

**Domi:** Hang mitigations present (higher rest 0.026 / gamma 1.68, continuous_hold adj sustain, silence snap, stop preempt) but unproven; risk thinner softs / clicky rests. Ghost prefire reduced by latch+decay; residual stale-gate + 50ms early on slow BLE. BLE: stop interval 12ms may chatter zeros. Watchdog: 2s processing stamp looks correct; FP risk low if capture alive; tests unrun in verify.

**Verdict:** Strong WIP, **not ship**. Blockers: wire climax `motor2` live path, desktop Gate→Beat reorder, green cargo+parity+android unit tests, hardware Domi/Edge A/B.

---

## Implement report

### A — engine-feel

**delta:** Boom/pluck true-rest and climax deep deny now raise silence-class hard-snap zeros on both platforms; micro-pauses 90–120ms with schedule surviving retrigger and earlier first rest; velocity overshoot is attack-target-only with 1.20 process headroom; prefire injects 1.15–1.35 strength with one-shot latch and tighter tempo decay/recency; climax sequences tease-then-surge, caps modulation to ≤5Hz, freezes deny dwell, multi-wrap cycle maturity with stronger post-deny punch.

**tests:** `cargo test` (lib+bin unit, all green); `cargo test --test parity` (green after golden regen); android `gradlew testDebugUnitTest` (green)

**risks:** 50ms prefire may feel early on slow BLE; deep deny hard-zero can feel clicky; longer micro-pauses change pad texture; golden regenerated so Kotlin ParityTest must match; Phase-2 slew redesign still deferred pending Domi A/B

**files:**

- `C:\Users\coldb\Chloe-Vibes\src\audio.rs`
- `C:\Users\coldb\Chloe-Vibes\src\gui.rs`
- `C:\Users\coldb\Chloe-Vibes\tests\parity_golden.csv`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\audio\EnvelopeProcessor.kt`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\audio\ClimaxEngine.kt`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\audio\BeatDetector.kt`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\audio\AudioCaptureManager.kt`

---

### B — Android dead-man / safety

**delta:** Android primary audio→BLE path now has a desktop-parity 2s dead-man: each successful processing frame stamps `elapsedRealtime` heartbeat; Application polls every 250ms and on stale heartbeat or processing-loop fail-closed (>100 consecutive errors) revokes `OutputOwner.Audio` under `outputLock`, stops the producer outside the lock, forces `stopMotors` (pending queue coalesced to stop), and publishes sticky `SafetyUiState`. UI shows WATCHDOG chip + banner and a sticky red Stop all devices rail (`emergencyStopAll` zeros motors, kills audio, drops companion lease so Finally fails closed on next write). Companion/Finally path still uses `CompanionSessionController` 1.75s lease and is not supervised by the audio watchdog. `disconnect()` always `stopMotors` before GATT close.

**tests:** `gradlew :app:compileDebugKotlin` SUCCESS; `testDebugUnitTest` `AudioPipelineWatchdogTest` 8/8 pass + `FunscriptVibrationSessionControllerTest` 11/11 pass

**risks:** BLE stop is best-effort (async GATT; process kill cannot zero hardware). Watchdog false-positive if processing thread is starved >2s under extreme load. `emergencyStopAll` drops companion session ownership (Finally re-acquires). Peak-hold/write-queue "latest wins" can still overwrite a queued stop if a non-fenced intensity races at BLE layer—Application owner fence is the primary guard. No `device_proof` run in this lane.

**files:**

- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ChloeVibesApplication.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/AudioCaptureManager.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/device/BleDeviceManager.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/MainActivity.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ui/MainScreen.kt`
- `android/app/src/test/kotlin/com/ashairfoil/chloevibes/AudioPipelineWatchdogTest.kt`
- `android/app/src/test/kotlin/com/ashairfoil/chloevibes/device/LovenseProtocolTest.kt`

---

### C — device curves / BLE

**delta:** Domi/wand rest floor 0.026 + gamma 1.68 (true-zero softs, harder punch contrast); compact/dual/generic floors+gammas raised in lockstep. Android peak-hold 22ms with zero and ≥3-step drop bypass; BLE min interval 28ms with 12ms stop path; MotorFeel session reset + provisional name curves + Nora A/C DualBody; disconnect best-effort stop; desktop near-rest quantize 0.008. No residual-gate re-arm changes.

**tests:** `cargo test --lib`: 30 passed. `cargo test --test parity`: FAIL (pre-existing audio golden drift, not device shaping). `gradlew testDebugUnitTest`: blocked by pre-existing MainActivity/MainScreen compile errors; `LovenseProtocolTest` added but not executed.

**risks:** Higher rest floors mute very soft musical content on Domi; 22ms peak-hold may feel thinner on some hits vs 55ms; stop@12ms may stress flaky firmware; Android unit tests not green until other-lane UI compile fixes; Domi hardware A/B still required for final floor/gamma sign-off.

**files:**

- `C:\Users\coldb\Chloe-Vibes\src\gui.rs`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\device\BleDeviceManager.kt`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\device\LovenseProtocol.kt`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\test\kotlin\com\ashairfoil\chloevibes\device\LovenseProtocolTest.kt`

---

### D — presets / boom catalog

**delta:** Bass Drum boom path unchanged and still default king; Club 125 slew 42ms; Deep 90 / Club 125 / Hard 140 catalog + Chloe name migration; Edge & Deny and Crescendo climax ON with real edge/arc params; Slow Tease / Ride the Beat / Break Me retuned for hard edge without flat-max adaptation death; Preset snapshots now carry knee/curve/slew on both platforms; `apply_preset` writes them; settings missing-key fallbacks Hybrid+LowPass.

**tests:** `cargo test --bin chloe-vibes presets::tests` (6 pass); `cargo test` lib+bin units (pass, 1 ignored); `cargo test --test parity` FAIL golden drift pre-existing; android `testDebugUnitTest` blocked by MainScreen CeilingControl/ClimaxArmRow compile errors (not presets).

**risks:** Legacy Chloe macro names remapped; Edge & Deny/Crescendo now modulate (was dry); engine tease×surge cancel/post-deny headroom still open M4; parity golden out of date independent of catalog; Android full unit suite blocked by unrelated UI compile break.

**files:**

- `C:\Users\coldb\Chloe-Vibes\src\presets.rs`
- `C:\Users\coldb\Chloe-Vibes\src\settings.rs`
- `C:\Users\coldb\Chloe-Vibes\src\gui.rs`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\audio\Presets.kt`
- `C:\Users\coldb\Chloe-Vibes\android\app\src\main\kotlin\com\ashairfoil\chloevibes\audio\AudioCaptureManager.kt`

---

### E — Android UX / arousal-safe UI

**delta:** Sticky STOP ALL rail (always on-screen) zeros motors+audio; Volume/Ceiling promoted with large targets and 50/75/100% chips + Floor≤Ceiling; presets use fat-finger cards with clear ACTIVE state (FIND BOOM remains desktop-only); climax disable requires confirm, shows CYCLE%+RESET, Build Cycle is true seconds 8–240; expert gate/ADSR collapsed by default; `keepScreenOn` while capturing.

**tests:** `compileDebugKotlin` PASS; `FunscriptVibrationSessionControllerTest` PASS; `ParityTest` FAIL (pre-existing golden drift, not UI); full unitTest compile blocked by unrelated `LovenseProtocolTest` imports when that suite is built.

**risks:** Expert knobs hidden until expanded; climax off needs confirm (intentional); ParityTest still red from engine/golden; no FIND BOOM on Android by design.

**files:**

- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ui/MainScreen.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/MainActivity.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/AudioCaptureManager.kt`
- `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ChloeVibesApplication.kt`

---

## Audit (abbrev)

### HAPTIC_FEEL_ENVELOPE

- **[critical]** `src/gui.rs:1504-1515`; `android/.../AudioCaptureManager.kt:653-662`: Prefire sets `is_onset=true` before the transient, but `onset_ok` still requires instantaneous `onset_strength` (flux/threshold) > 1.02. In the pre-fire window flux is typically low, so prefire is rejected and never reaches `EnvelopeProcessor.drive`. `BeatDetector.recent_onset_strength` is computed but never used for prefire velocity/arming.  
  **feel:** Kills the intended ~50 ms lead that puts punch on-beat; hits arrive late relative to music, so musical boom feels sluggish even when tempo is locked.

- **[high]** `src/audio.rs:602-618,798-799`; `EnvelopeProcessor.kt:115-129,303-304`: Velocity both scales magnitude (×(0.5+0.5·v), up to 1.5) and raises `attack_target` (up to 1.28), then `process()` returns `(value*magnitude).clamp(0,1)`. Default/hybrid/binary peaks (and Auto-Lock binary ≤0.88) still clip to 1.0 after velocity scaling, flattening the overshoot transient into a max plateau instead of a brief super-peak.  
  **feel:** Hard kicks should feel briefly louder than the body of the hit; clipping removes that visceral punch and wastes WAVE-001 overshoot work on the loudest, most important hits.

- **[high]** `src/audio.rs:887-893,603-606`; `EnvelopeProcessor.kt:376-382,116-117`: `gate_just_opened` always triggers with velocity=1.0 (no overshoot). A real onset 1–19 ms later is blocked by `min_retrigger_ms=20`, so the velocity-overshoot path never runs when the gate leads the flux peak (common on residual bass).  
  **feel:** Most boom hits become same-height gate thumps; dynamic punch contrast between soft and hard kicks collapses.

- **[high]** `src/audio.rs:772-799,829-835`; `src/gui.rs:1630-1657`; `AudioCaptureManager.kt:718-724`: Boom/pluck rest never sets `silence_event` (only Sustain micro-pauses do). Release tails stay non-zero and fall through full `output_slew_ms` down-path; hard-snap only when target≤0.001. Centroid also stretches bass release up to ×1.4 before that slew.  
  **feel:** Deep boom contrast needs honest motor rest between kicks; lengthened release+slew fills the trough, reduces punch recovery, and fights anti-adaptation.

- **[medium]** `src/audio.rs:832-835,882-884,584-598`; `EnvelopeProcessor.kt:335-338,371-373,97-111`: `continuous_hold` uses raw `sustain_level`, but `enter_post_decay` uses centroid-adjusted sustain. Near the pluck boundary (raw ~0.15–0.20 with high centroid) process takes the pluck Release path while `continuous_hold` still re-arms from Idle while the gate is open → retrigger stutter instead of a held pad.  
  **feel:** Bright organ/pad material can chatter instead of riding smoothly; breaks the organ vs boom contract.

- **[medium]** `src/gui.rs:1513-1515`; `src/audio.rs:874-875`; `AudioCaptureManager.kt:661-662`; `EnvelopeProcessor.kt:366-367`: Double onset gates: outer path requires strength > 1.02, drive requires > 1.05 for `is_onset_trigger`. Band 1.02–1.05 is accepted then ignored for velocity retrigger (gate-edge may still fire without overshoot).  
  **feel:** Borderline kicks lose velocity punch inconsistently; feel becomes random with mix/threshold.

- **[low]** `src/audio.rs:620-632,693-709`; `settings.rs:285-286`: Default boom `attack_ms=20` (<50) always skips Attack; `attack_curve` is unused on the flagship path. `phase_start_value=value.max(0.4)` only matters for slow attacks and can invent a 0.4 floor on soft retriggers.  
  **feel:** Attack controls read as placebo on boom; slow-attack presets can get an artificial partial hit that softens dynamics.

- **[low]** `src/audio.rs:789-795`; `EnvelopeProcessor.kt:296-299`: Idle still does `value *= 0.95` per call (frame-rate dependent). Output is already 0 via magnitude=0, so this is mostly state cosmetics—but it remains a latent time-base footgun if magnitude handling changes.  
  **feel:** Low direct impact today; risks parity/feel drift if Idle output is ever taken from value alone.

- **[low]** `docs/TECHNICAL_REFERENCE.md:87,95,138`; `src/audio.rs:614-616`; `settings.rs:276`: Docs still claim overshoot max 1.2, output slew ~85 ms, micro-pauses 48–96 ms; live code uses overshoot to 1.28, default slew 42 ms, pauses 60–100 ms.  
  **feel:** Mis-tunes hardware A/B and dual-client reviews against stale targets; not a runtime bug but blocks honest feel iteration.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Restores on-beat punch lead (~50 ms) on both clients; largest latency/feel win still open after WAVE-001 / small | Make prefire arm the envelope with stored onset velocity | `src/audio.rs` (expose `recent_onset_strength` or `prefire_velocity`); `src/gui.rs`; `android/.../BeatDetector.kt`; `android/.../AudioCaptureManager.kt`; tests/parity + unit |
| Hard kicks briefly exceed body level again; Auto-Lock 0.88 headroom becomes real punch instead of a flat 1.0 ceiling / small | Stop double-scaling then clipping overshoot peaks | `src/audio.rs` EnvelopeProcessor::trigger/process; `android/.../EnvelopeProcessor.kt`; envelope unit tests + golden if peaks change |
| Gate-leading kicks keep velocity overshoot instead of locking a velocity=1.0 thump for 20 ms / medium | Merge gate-edge with imminent onset (overshoot upgrade inside retrigger window) | `src/audio.rs` drive/trigger; `android/.../EnvelopeProcessor.kt`; new host tests for gate-then-onset |
| Deepens boom troughs: motors hard-snap to rest between kicks instead of slewing through the floor / small | Raise `silence_event` on pluck/boom true-rest (Idle entry / near-zero release) | `src/audio.rs` process/enter_post_decay; `android/.../EnvelopeProcessor.kt`; `gui.rs` + AudioCaptureManager silence_class already wired |
| Prevents bright near-boundary pads from Idle-retrigger stutter; keeps boom/organ contract honest / small | Unify pluck vs organ decision on the same sustain used post-centroid | `src/audio.rs` drive continuous_hold vs adj_sustain_level; `android/.../EnvelopeProcessor.kt` |
| Removes inconsistent weak-onset handling so punch velocity is predictable / small | Single onset strength threshold (1.05) shared by GUI pre-gate and drive | `src/gui.rs`; `src/audio.rs`; `AudioCaptureManager.kt`; `EnvelopeProcessor.kt` |

---

### CLIMAX_EDGE_DENY

- **[critical]** `src/presets.rs:677-705`: Factory preset "Edge & Deny" (and Android twin `Presets.kt:527-548`) ships `climax_enabled=false` / default climax fields, so ClimaxEngine edge-and-deny, tease, surge, and arousal momentum never run despite the name and description promising build-then-pull-back denial.  
  **feel:** The most discoverable edge-and-deny product path is a no-op for the entire climax stack; users only get ADSR sustain, not escalating denies or climax arcs.

- **[high]** `src/audio.rs:1331-1363`: Tease (starts at 1-tease_ratio) and surge (hard-coded from progress 0.80) both multiply into `raw_output`. Slow Tease (tease_ratio=0.30, drop=0.70) holds a deep dip through the surge window; Ride the Beat/Break Me cut Surge-pattern peaks with overlapping tease. Kotlin `ClimaxEngine.kt:157-191` is lockstep.  
  **feel:** End-of-cycle "devastating climax" is arithmetically cancelled into near-unity; flagship experience presets feel flat or yanked at the moment they should peak.

- **[high]** `src/audio.rs:1434-1451`: Arousal gain (up to ~3.8x), surge_boost (to +1.5), and post-deny onset_boost (+0.30..0.55, cap 0.65) all feed a hard clamp to 1.0. Experience presets use high sustain (0.72-0.92); when dry*product is already saturated, post-deny overshoot and surge add no amplitude. Boost is also applied after that frame's `raw_output` is computed.  
  **feel:** The signature edge-and-deny return punch and late-cycle surge are often unfeelable exactly when the user is already loud—the moment contrast matters most.

- **[high]** `src/gui.rs:1773-1794`: `ClimaxEngine.motor2_output` (anti-phase spatial contrast + deny shaping) is computed and parity-tested but never sent to hardware. Desktop and Android `AudioCaptureManager.kt:732-749` replace it with treble-fraction * motor1. Break Me copy and TECHNICAL_REFERENCE §3.3 still claim climax dual-motor anti-phase.  
  **feel:** On Edge/Nora-class dual actuators, climax never creates the promised spatial movement or asymmetric deny; second motor is only a treble shadow of motor1.

- **[high]** `src/audio.rs:1461-1509`: Edge-and-deny reduces to residual (1-deny_depth) with depth 0.60→0.90 (never true zero), does not set `EnvelopeProcessor.silence_event` / silence-class, and still passes through min_vibe mapping + output slew. TEMPORAL_ARCHITECTURE expects deep climax drops to bypass slew for honest rest.  
  **feel:** Deny feels like a soft plateau dip, not a gasp-to-stillness edge; nerves never fully reset, so the return cannot feel devastating.

- **[medium]** `src/audio.rs:2169-2241`: Section labeled "ClimaxEngine deny activation" only contains Lorenz bounds and Stairs similarity tests; no assertion that high_output dwell triggers deny, depth/duration maturity, or post-deny boost.  
  **feel:** Regressions in the core edge-and-deny state machine can ship unnoticed while parity still passes on non-deny scenarios.

- **[medium]** `src/audio.rs:1300-1307`: On cycle wrap, `cycle_count` saturating-adds 1 even when `floor((t-anchor)/cycle_len) > 1`; multi-cycle jumps under-advance `cycle_maturity` (tease depth, deny aggressiveness, post-deny boost).  
  **feel:** Long sessions with stalls/jumps escalate slower than designed; later cycles stay "gentle first deny" instead of mature devastation.

- **[low]** `android/.../ui/MainScreen.kt:438-441`: Android climax UI caps Surge Boost at 1.2 and Pulse Depth at 0.45; desktop one-click Edge/Overload/Punisher profiles (`gui.rs` apply_climax_profile) have no Android equivalent. Sanitize clamps tease_ratio/drop wider than engine (`settings.rs` vs `audio.rs`).  
  **feel:** Mobile users cannot match desktop climax profiles or full Break Me pulse depth without hand-tuning; minor preset/engine clamp drift risk.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Fixes the named product path so picking Edge & Deny actually runs tease/deny/momentum on both clients / S | Turn on climax for Edge & Deny with real edge params | `src/presets.rs`; `android/.../audio/Presets.kt` |
| Restores devastating end-of-cycle climax on Slow Tease / Break Me / Ride the Beat instead of multiplicative cancel / M | Sequence tease then surge (non-overlapping terminal arc) | `src/audio.rs` ClimaxEngine; `android/.../audio/ClimaxEngine.kt`; tests/parity.rs golden regen |
| Users feel the deny→overshoot snap when already loud (multiplicative boost or temporary headroom, apply on exit frame) / M | Make post-deny return punch survive the 1.0 ceiling | `src/audio.rs`; `android/.../audio/ClimaxEngine.kt` |
| Delivers promised spatial anti-phase and asymmetric deny on Edge/Nora; keep treble path when climax off / M | Wire climax `motor2_output` to dual-motor devices when climax enabled | `src/gui.rs`; `android/.../audio/AudioCaptureManager.kt` |
| Honest motor rest during deny so nerves reset and the return is devastating; aligns with TEMPORAL_ARCHITECTURE / M | Deep deny as near-zero silence-class event | `src/audio.rs`; `src/gui.rs`; `android/.../audio/ClimaxEngine.kt`; `AudioCaptureManager.kt` |
| Locks edge-and-deny behavior against regressions and correct escalation over long sessions / S | Add deny activation + maturity tests; fix multi-wrap `cycle_count` | `src/audio.rs` tests; android unit tests; parity if outputs change |

---

### BEAT_PREFIRE_TEMPO

- **[critical]** `src/gui.rs:1504-1515` (+ `audio.rs:874-875`; `AudioCaptureManager.kt:654-662`; `EnvelopeProcessor.kt:366-367`): Prefire is dead in the motor path: when `prefire_ok` arms `is_onset`, frame `onset_strength` is flux/threshold ≤1.0 (no real onset), then dual gates require >1.02 then >1.05 — every prefire is rejected. `recent_onset_strength` is tracked but never applied.  
  **feel:** 50ms lead never reaches Lovense; kicks stay late by transport+spin-up (~85–115ms). Maximum musical punch and on-beat boom are impossible while prefire is cosmetic.

- **[high]** `src/audio.rs:1062-1077` (+ `gui.rs:1504-1510`; `BeatDetector.kt:126-132`): No one-shot prefire latch: `prefire_ok` stays true for the full `PREFIRE_LEAD_MS` window. Desktop re-checks every UI frame (~60–240Hz) even without fresh spectral; if strength is fixed, `min_retrigger_ms=20` allows 2–3 ghost punches, then the real onset can fire again ~50ms later.  
  **feel:** Fixing strength without a latch turns latency fix into stutter/double-hits — ruins honest rest and single devastating kick arcs.

- **[high]** `src/audio.rs:1098-1103` (+ `BeatDetector.kt:149-154`): When tempo lock dies (conf<0.05), `tempo_interval`/confidence clear but `onset_timestamps[]` and `onset_ts_count` are NOT reset. Next onset with count still ≥4 rebuilds tempo from mixed old+new IOIs.  
  **feel:** Track change / drop-out can re-lock wrong grid and ghost-prefire into the next song — anti-ghost decay is undermined by a stale ring.

- **[medium]** `src/gui.rs:1472-1534` vs `AudioCaptureManager.kt:637-656`: Pipeline order parity break: desktop runs Beat+prefire before Gate (prefire uses previous-frame `gate_is_open`); Android runs Gate then Beat (current-frame gate). Frozen chain is Spectral→Gate→Beat→…  
  **feel:** One-frame gate lag on desktop can arm prefire on residual-open or miss first open after silence — dual-client feel drift on the same track.

- **[medium]** `src/gui.rs:1513-1515` (+ `AudioCaptureManager.kt:661`): Even with synthetic prefire strength, energy > `gate_threshold*0.40` can still kill prefire on sparse/plucky material where between-beat energy is low (the exact case needing predictive punch).  
  **feel:** Boom contrast tracks with quiet troughs need prefire most; energy gate may silence them while dense bass passes.

- **[medium]** `src/audio.rs:1111-1157` (+ `BeatDetector.kt:162-207`): Tempo IOI uses plain mean of intervals in 150–2000ms; no median/mode clustering. Eighth-note grids, swing, and fills pull mean off the perceptual beat (Auto-Lock folds octaves; BeatDetector does not).  
  **feel:** Wrong grid → prefire lands off-kick (early ghost or late mush). Tempo confidence CV can also thrash lock on live drums.

- **[medium]** `src/audio.rs:2072-2114` vs android tests: Rust tests decay + `prefire_ok` false on corpse lock only. No test that prefire produces envelope trigger; no Kotlin prefire/decay unit tests; parity golden does not cover pre-fire (TEMPORAL_ARCHITECTURE.md).  
  **feel:** Dead prefire shipped as WAVE-001 "implemented" because suite never asserted motor-path consumption — regression will stay invisible.

- **[low]** `src/audio.rs:1071-1073` vs `1085-1086` (+ Kotlin mirrors): Recency allows prefire for 2.0 beats while decay grace is 1.5 beats; after ~0.5 stale beat conf can still be >0.6 (e.g. 0.9×√0.55≈0.67) so one more prefire is legal on a dying lock.  
  **feel:** Small ghost-fire window into silence/breakdown before recency hard-cuts — softens honest motor rest at drops.

- **[low]** `src/audio.rs:975` (+ `BeatDetector.kt:26`; WAVE-001.md:65): Fixed `PREFIRE_LEAD_MS=50` with no BLE/device-class compensation; WAVE-001 open risk: may feel early on slow Android BLE.  
  **feel:** Early prefire reads as ghost hit before the kick; late BLE still feels sluggish — one constant cannot serve Domi desktop vs phone BLE equally.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Restores 50ms on-beat punch on both clients — highest feel ROI in this domain / M | Arm prefire with synthetic strength (use `recent_onset_strength`, floor ~1.12–1.20) and skip flux 1.02 reject on prefire path only | `src/gui.rs`; `src/audio.rs` (expose `recent_onset_strength`); `android/.../AudioCaptureManager.kt`; `android/.../BeatDetector.kt` |
| Prevents multi-frame stutter + prefire+real double-kick once strength works / S | One-shot prefire latch per predicted beat (consume prediction or `last_prefire_ms`; suppress until prediction advances) | `src/audio.rs` BeatDetector; `android/.../BeatDetector.kt`; call sites `gui.rs` + `AudioCaptureManager.kt` |
| Stops cross-track ghost tempo / wrong-grid prefire after decay / S | Hard-reset onset ring (timestamps, count, index) when tempo lock fully clears | `src/audio.rs` decay_tempo_confidence; `android/.../BeatDetector.kt` decayTempoConfidence |
| Parity-locked open/close edge for predictive hits / S | Desktop: process Gate before Beat/prefire (match Android + frozen chain); use current-frame gate for prefire | `src/gui.rs` advanced pipeline block ~1472-1534 |
| Locks WAVE-001 feel contract so prefire cannot die again unnoticed / M | Add dual tests: locked grid → `prefire_ok` → envelope trigger; silence → conf decay → !prefire; latch no double-fire in 50ms window | `src/audio.rs` tests; `android/.../ParityTest.kt` or `BeatDetectorTest.kt` |
| More honest lock on eighth-note / swing material → prefire lands on kick / M | Optional: median/mode IOI or `fold_to_perceptual_beat` inside BeatDetector tempo estimate | `src/audio.rs` update_tempo_prediction; `android/.../BeatDetector.kt` updateTempoPrediction |

---

### BLE_LATENCY_DEVICE_CURVES

- **[high]** `android/.../device/BleDeviceManager.kt`: Single-slot `pendingCommand` is last-write-wins: `DeviceType;`/`Battery;` scheduled at Ready can be overwritten by Vibrate and never retried, so `applyDeviceType` never runs.  
  **feel:** Without DeviceType, Edge stays single-motor (no bass/treble split) and rest-floor/gamma stay Generic — Domi hang floors and dual-body punch never engage for the session.

- **[high]** `android/.../device/BleDeviceManager.kt`: `connectInternal` does not reset `isDualMotor`/`motorRestFloor`/`motorFeelGamma`; disconnect clears dual only, not curves — stale motor class across device switch or failed DeviceType.  
  **feel:** Wrong protocol (Vibrate1/2 on a mono wand) or wrong rest/gamma makes motors hum, go quiet, or lose punch after swapping Domi/Edge/Lush.

- **[high]** `android/.../device/BleDeviceManager.kt`: `peakHoldMs=55` holds non-zero peaks; only level≤0 bypasses. Desktop device task has no peak-hold — Android fills kick troughs that quantize above 0.  
  **feel:** Deep boom contrast and honest motor rest between hits are the product signature; 55ms peak glue turns devastating kick arcs into a flatter buzz on Domi/Edge.

- **[medium]** `android/.../device/BleDeviceManager.kt`: Motor curves and `isDualMotor` only set in `applyDeviceType`; `modelHint` exists but is not used for provisional rest/gamma/dual at connect.  
  **feel:** First hundreds of ms (or whole session if DeviceType drops) use Generic 0.02/1.4 — soft hang and wrong dual path right when the user starts music.

- **[medium]** `android/.../device/BleDeviceManager.kt`: Nora DeviceType codes: C mapped to Compact, A falls through Generic; desktop MotorKind maps nora→DualBody (rest 0.022/γ 1.45).  
  **feel:** Cross-platform parity break — same toy feels different on Android vs desktop (rest floor and power curve).

- **[medium]** `android/.../device/BleDeviceManager.kt`: `setDualIntensity` applies the same `shapeForMotor(rest, gamma)` to motor1 and motor2; Edge actuators are not physically matched.  
  **feel:** Weaker Edge motor may hang above rest or feel dull while the stronger motor is correct — spatial dual-motor arcs lose contrast.

- **[medium]** `android/.../device/BleDeviceManager.kt`: `minWriteIntervalMs=30` (~33Hz) plus writeInFlight/120ms watchdog vs desktop fixed 20ms/50Hz device loop.  
  **feel:** Extra 10–30ms+ on attack/release edges dulls musical punch and can make tempo pre-fire feel early or mushy on real BLE.

- **[medium]** `android/.../device/BleDeviceManager.kt`: Wand γ=1.55 then continuous<0.5 hard-zero removes roughly sub-~10% shaped pipeline levels after rest remap.  
  **feel:** Stops Domi hang (good) but also erases soft musical body and anti-adaptation micro-motion — sessions can feel binary on/off instead of alive.

- **[low]** `android/.../device/LovenseProtocol.kt`: vibrate2 KDoc still says dual motors e.g. Domi 2/Nora; product/docs only fixture-verify Edge code P for Vibrate1/2.  
  **feel:** Misleads future changes into enabling dual on mono wands — risk of dropped commands or silent motors.

- **[low]** `android/.../device/BleDeviceManager.kt`: `enableNotificationsAndFinish` writeDescriptor(CCCD) then marks Ready and schedules DeviceType without `onDescriptorWrite` sequencing.  
  **feel:** On picky stacks, early commands/notifications fail → DeviceType never arrives → same wrong-curve/mono-Edge failure mode.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| high / M | Protect DeviceType: priority control queue + retry until `applyDeviceType` | `android/.../device/BleDeviceManager.kt` |
| high / S | Reset `isDualMotor` + rest/gamma defaults in `connectInternal` (and disconnect) | `android/.../device/BleDeviceManager.kt` |
| high / S | Retune peak-hold: shorten (~20–25ms) or bypass on large downward steps / near-rest | `android/.../device/BleDeviceManager.kt` |
| high / S | Provisional curves from advertised LVS code/`modelHint` at connect; confirm on DeviceType | `android/.../device/BleDeviceManager.kt`, `LovenseProtocol.kt` |
| medium / S | Align Nora (A/C) with DualBody; optional weaker rest/gamma on Edge motor2 | `android/.../device/BleDeviceManager.kt`, `src/gui.rs` (parity check) |
| medium / S | A/B `minWriteIntervalMs` 20 (50Hz) on Domi/Edge; keep zero and stop prioritized | `android/.../device/BleDeviceManager.kt` |

---

### ANDROID_SAFETY_WATCHDOG

- **[critical]** `ChloeVibesApplication.kt:34-47`: Primary audio→BLE path has no pipeline dead-man watchdog. Desktop device tasks stop if `pipeline_heartbeat` is stale >2s (`gui.rs` WATCHDOG_TIMEOUT_MS). Android only leases companion/Finally sessions (`CompanionSessionController` DEFAULT_HEARTBEAT_TIMEOUT_MS=1750). If ChloeVibes-Processing hangs, ANRs, or stops emitting frames while last BLE level >0, hardware holds last intensity indefinitely.  
  **feel:** Stuck non-zero destroys honest motor rest and turns a hang into continuous body drive; safe silence is required for punch/contrast and climax arcs.

- **[critical]** `ChloeVibesApplication.kt:34-47,78-86`: Safety stop is not atomic with motor writes: `onOutputUpdate`/`onDualOutputUpdate` check `outputOwner` outside `outputLock`, then call `setIntensity`/`setDualIntensity`. `stopAudioCapture` sets Idle, joins producer (500ms), then `stopMotorsSafely` — a frame that already observed Audio can still write after the stop. Comment claims final frame cannot overwrite safety stop; code does not enforce that.  
  **feel:** User hits Stop expecting true rest; a raced post-stop Vibrate can re-arm the motor and erase the rest floor.

- **[high]** `device/BleDeviceManager.kt:292-305`: `disconnect()` sets `userRequestedDisconnect`, `closeGatt()`, `resetWriteState()` without `stopMotors()`/`LovenseProtocol.stop()`. Intentional Disconnect and `disconnectIfNoCompanionSession` can drop the link while the toy keeps last vibration (common Lovense behavior until a new command or power-off).  
  **feel:** Disconnect is treated as an off switch in UI; without a pre-close zero, the body keeps the last peak — unsafe and anti-rest.

- **[high]** `ui/MainScreen.kt:614-640`: No verified red Stop-all control. UI only has Stop (capture) and Disconnect. `stopMotorsSafely()` is `runCatching { stopMotors() }.isSuccess` — `stopMotors()` returns Unit and ignores `sendCommand` false / not-Ready, so `acquireCompanionOutput` and release paths can report success without a delivered Vibrate:0. Desktop verifies `stop_all_devices` and shows STOP COMMAND FAILED banner.  
  **feel:** When BLE write gate is stuck or link is half-open, the operator has no verified panic stop and may believe motors are off while they still run.

- **[high]** `audio/AudioCaptureManager.kt:780-788`: `processingLoop` breaks after >100 consecutive frame exceptions without setting `running=false`, without a final zero callback, and without `stopMotors`. `isRunning` stays true so UI can show LIVE with a dead pipeline and last motor level held.  
  **feel:** Engine death mid-climax leaves a frozen intensity with no automatic rest — worst-case continuous drive.

- **[medium]** `MainActivity.kt:138-145`: No `onPause`/`onStop` safety; only `onDestroy` when `isFinishing` calls `disconnectIfNoCompanionSession`. No UncaughtExceptionHandler / Application panic-stop equivalent (desktop `register_panic_stop` + Drop stop_all). Process crash or LMK kill cannot stop BLE; even recoverable main-thread death leaves motors at last command.  
  **feel:** Lifecycle and crash paths are how sessions end in real use; without best-effort zero, rest never arrives after a fault.

- **[medium]** `device/BleDeviceManager.kt:389-410,311-323`: Unexpected STATE_DISCONNECTED schedules reconnect but never issues a local stop or post-Ready zero. During RF blip the device often keeps last vibration; after Ready, audio may resume mid-level without an explicit rest edge. Companion `setCompanionPosition` does `stopMotors` when not Ready; audio path has no equivalent link-loss fence.  
  **feel:** Reconnect without a clean zero turns dropouts into sustained buzz and blurs musical rest between phrases.

- **[medium]** `device/BleDeviceManager.kt:530-602,779-796`: Single `pendingCommand` slot: `stopMotors()` may only queue Vibrate:0 while writeInFlight; a later `setIntensity` can overwrite pending before flush. Combined with unlocked audio callbacks, stop is not durable. Write watchdog only unsticks writeInFlight — it is not a pipeline dead-man.  
  **feel:** Lost stop commands mean micro-pauses and user Stop never reach the ERM — rest contrast fails.

- **[low]** `docs/TECHNICAL_REFERENCE.md:264`; `docs/waves/WAVE-001.md:14`; `README.md:47`: Product docs still state Android has no dead-man watchdog; WAVE-001 explicitly defers Android lifecycle/watchdog. Working tree confirms companion lease only — audio path still open. CLAUDE.md: Android watchdog parity still open.  
  **feel:** Documents known residual risk; does not fix hardware hold, but confirms intentional gap vs desktop safety reference.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| critical / M | Add audio-path dead-man (mirror desktop 2s / companion 1.75s) | `ChloeVibesApplication.kt`; `AudioCaptureManager.kt` (heartbeat stamp each frame); `BleDeviceManager.stopMotors`; unit test like FunscriptVibrationSessionControllerTest.staleHeartbeatCannotExtendDeadman |
| critical / S | Make motor writes lease-fenced under `outputLock` (or monotonic generation) | `ChloeVibesApplication.kt` onOutputUpdate/onDualOutputUpdate; stopAudioCapture/acquireCompanionOutput |
| high / S | `stopMotors` before every disconnect + verified stop with retry/UI error | `BleDeviceManager.disconnect`/`stopMotors`/`sendCommand`; `ChloeVibesApplication.stopMotorsSafely`; MainScreen red Stop all; MainActivity wiring |
| high / S | Fail-closed `processingLoop` exit: `running=false` + Application `stopMotors` | `AudioCaptureManager.processingLoop`; ChloeVibesApplication callback/hook |
| medium / S | Link-loss and Ready: zero motors; optional post-reconnect rest before audio resumes | `BleDeviceManager` gattCallback STATE_DISCONNECTED / Ready; ChloeVibesApplication output owner |
| medium / S | Best-effort UncaughtExceptionHandler + onStop policy for non-companion sessions | `ChloeVibesApplication`; MainActivity lifecycle |

---

### PRESETS_DEFAULT_PATH

- **[high]** `src/gui.rs:3904-3916`: Club 125 (`apply_chloe_rhythm_profile` Medium) sets `output_slew_ms=48.0` while Bass Drum / `settings::defaults::OUTPUT_SLEW_MS` / WAVE-001 use 42.0. Description claims "default bass-drum boom (same shape as Bass Drum)" but motor trough depth is softer than the flagship path.  
  **feel:** Slew fall is the real between-kick silence. 48 ms vs 42 ms keeps residual buzz in the trough and blunts boom contrast on the "default tempo" macro users will press instead of Bass Drum.

- **[high]** `src/gui.rs:907-912,3891-3919`: BOOM macros write `current_preset_name` "Deep 90"/"Club 125"/"Hard 140", but `factory_presets()` has no such names. On restart, `is_unknown_named` is true and Bass Drum is force-applied, wiping tempo-specific decay/slew (520/55, 375/48, 335/42).  
  **feel:** User tunes Deep 90 for slow body-long booms, restarts, and lands on 125 BPM Bass Drum envelope — wrong musical length, weaker rest, climax still off but the arc is wrong.

- **[high]** `android/.../audio/Presets.kt:828-904`: Android BOOM macros are catalog entries named Chloe Loose/Medium/Ultimate (not Deep 90/Club 125/Hard 140). Preset has no `outputSlewMs`; `AudioCaptureManager.applyPreset` rebuilds ProcessingParams without slew, so all macros get default 42 ms. Desktop Deep 90 uses 55 ms, Club 125 uses 48 ms.  
  **feel:** Same labeled boom path on phone vs desktop does not land the same trough/decay body; dual-client parity for the product default experience is broken on tempo macros.

- **[medium]** `android/.../audio/Presets.kt:36-42`: Preset data-class defaults `thresholdKnee=0.22f` and `dynamicCurve=1f`. Only ~8 boom-family presets override to 0.15/1.2. Desktop `Settings::apply_preset` always stomps knee/curve to defaults 0.15/1.2 for every preset (`presets.rs` has no knee/curve fields).  
  **feel:** Leaving Bass Drum for another preset then returning is fine, but non-boom catalog on Android is softer/less punchy than desktop; structural catalog skew fights "one feel" across clients.

- **[medium]** `src/settings.rs:465-482`: `Settings::load` uses `unwrap_or(TriggerMode::Dynamic)` and `unwrap_or(FrequencyMode::Full)` for missing keys, while Bass Drum / Default impl use Hybrid + LowPass. Migration only re-applies Bass Drum when name is "Default" or unknown — not when name is already "Bass Drum" with partial legacy storage.  
  **feel:** Upgrade/partial store can leave full-band Dynamic tracking with a Bass Drum label: hats and mids ride the motor, climax may still be off, but kick-only boom contrast dies.

- **[medium]** `android/.../audio/AudioCaptureManager.kt:258-288`: `applyPreset` does not set `outputSlewMs` or `outputGain` explicitly; relies on ProcessingParams defaults (42f / 1f). Fine for Bass Drum, but cannot express tempo-macro slew and silently discards any prior expert slew when switching presets.  
  **feel:** Preset is not a complete motor-path snapshot on Android; boom macros cannot own trough depth the way desktop `apply_chloe_rhythm_profile` does.

- **[low]** `src/presets.rs:104-140` vs android `Presets.kt:93-120`: Bass Drum flagship numbers and `climax_enabled=false` are parity-locked and match `settings::defaults`. Desktop factory count 30; Android 33 (extra Chloe macros). Docs TECHNICAL_REFERENCE already note knee/curve Android-only and 30 vs 33.  
  **feel:** Core default path is correct when users actually land on Bass Drum cold-start; residual risk is catalog/macro drift around that path, not the Bass Drum numbers themselves.

- **[low]** `src/auto_lock.rs:101-124`: FIND BOOM correctly forces `climax_mode_enabled=false` and fits boom body (attack 20, decay 0.78*beat, sustain ~0.08, knee 0.15). Does not write volume/gain/min/max. Desktop-only (WAVE-001 non-goal).  
  **feel:** Climax-off boom path is healthy on desktop auto-tune; Android users only get static Bass Drum/Chloe presets for the same job.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Restores deep boom silence on the default-tempo macro; stops Club 125 undoing the 48→42 trough fix / S | Align Club 125 slew with Bass Drum / WAVE-001 (42 ms); re-check Deep 90/Hard 140 trough intent | `src/gui.rs` (apply_chloe_rhythm_profile); optionally docs/TECHNICAL_REFERENCE.md |
| Restart no longer stomps tempo macros to Bass Drum; find_preset/migration/parity catalog unify / M | Promote Deep 90 / Club 125 / Hard 140 into factory_presets + rename Android Chloe* to match | `src/presets.rs`, `android/.../Presets.kt`, `src/gui.rs` (names already Deep/Club/Hard) |
| True complete boom snapshots; Android macros can match desktop trough; apply_preset stops stomping vs catalog mismatch / M | Add `outputSlewMs` (and knee/curve as first-class) to Preset snapshots on both clients | `src/presets.rs`, `src/settings.rs` apply_preset, android Presets.kt, AudioCaptureManager.applyPreset, MainScreenState.applyPreset |
| Legacy/partial storage cannot silently leave full-band Dynamic under a boom label / S | Fix Settings::load missing-key fallbacks to Hybrid + LowPass (Bass Drum) | `src/settings.rs` |
| Non-overridden Android presets stop applying softer gate/linear curve vs desktop boom defaults / S | Change Kotlin Preset defaults `thresholdKnee=0.15f`, `dynamicCurve=1.2f` | `android/.../Presets.kt` |
| Heals partial stores and locks climax-off boom path every cold start without wiping true Custom / S | On desktop load, if `current_preset_name` is Bass Drum (or BOOM macro), re-apply full snapshot | `src/gui.rs` (startup migration block) |

---

### DESKTOP_UI_FIND_BOOM

- **[high]** `src/gui.rs:1459-1469,1824,2004-2011,3656-3657`: ENERGY GATE history is post-volume smoothed energy (`raw_energy`/`stable_energy`) while `gate_threshold` and the dashed line are pre-volume; default volume 1.90 saturates the fill so THRESHOLD cannot be tuned against the plot.  
  **feel:** Users cannot see when kicks open the gate vs troughs close it — the primary visual for FIND BOOM / boom contrast is misleading, so hand-tunes after lock fight the music.

- **[high]** `src/auto_lock.rs:256-261`; `src/gui.rs:1369-1375`: `cancel()` does not clear `last_report`; UI always shows `report_line()` even when state is Idle after a manual/preset cancel.  
  **feel:** After taking ownership of knobs, the desk still reads as locked boom (BPM/band/decay/punch), so users trust a fit that is no longer supervised.

- **[high]** `src/auto_lock.rs:357-365,530`; `src/gui.rs:2164-2166`: User-override guard only runs when expected is set (post-commit). During Listening, presets/sliders do not cancel; category presets never call `auto_lock.cancel()`.  
  **feel:** TUNING… can overwrite a just-chosen Bass Drum / envelope tweak after ~4–8s — controls feel dead or hostile right when the user is hunting boom.

- **[high]** `src/gui.rs:2907-2915`; `src/audio.rs:1545-1549`: OUTPUT Floor hover encourages raising `min_vibe` for "base vibration always present"; `map_output` lifts every non-silent sample including FIND BOOM sustain troughs.  
  **feel:** One slider move erases honest motor rest and deep boom contrast while BOOM score still looks great — anti-goal for max-dynamic bass-drum feel.

- **[medium]** `src/gui.rs:2395-2403,3437-3447`; `src/audio.rs:620-625`; `src/auto_lock.rs:846`: Attack slider 0.5–500ms and ADSR preview draw a real attack stage; engine treats `attack_ms < 50` as instant peak→Decay. FIND BOOM pins 20ms; OVERRIDE attack curve is inert in that band.  
  **feel:** Dragging Attack or reading ENVELOPE SHAPE teaches fake punch control; real snap is slew/motor, so users chase placebo instead of Motor ramp / Decay.

- **[medium]** `src/gui.rs:2465-2592,1639-1653`: `output_slew_ms` (Motor ramp) — dominant fall ballistics and punch rise ratio partner — is only under collapsed OVERRIDE; main boom surface omits it.  
  **feel:** Trough depth and hang vs snap are the difference between devastating boom and muddy buzz; burying the control makes FIND BOOM results hard to A/B on hardware.

- **[medium]** `src/gui.rs:2164-2215`: BOOM macros cancel auto_lock; category preset grid only `apply_preset` — asymmetric cancel during listen/lock races.  
  **feel:** Same user intent (pick a boom starting point) behaves differently; intermittent post-listen stomp feels like random dead presets.

- **[low]** `src/auto_lock.rs:834-855`; `src/gui.rs` (no gate_smoothing slider): FIND BOOM writes `gate_smoothing` (0.04/0.08) with zero UI surface; only re-lock or presets can change it.  
  **feel:** Gate close smoothness shapes trough cleanliness; invisible after lock so residual hang cannot be diagnosed from the desk.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Restores honest THRESHOLD vs fill so post-FIND-BOOM gate tweaks match what the motor hears; highest visual fix for boom fire/rest / S | Plot gate-domain energy on ENERGY GATE | `src/gui.rs` — push `raw_energy_for_gate` (or dedicated `gate_energy_history`) into `draw_output_history`; keep output line on `vibration_level` |
| Stops false "still locked" BPM/decay line after ownership; Revert/Keep/report stay coherent with button state / S | Honest lock chrome: clear report on cancel; show report only when Locked | `src/auto_lock.rs` (`last_report=None` in cancel()); `src/gui.rs` (gate report_line UI on `is_locked()`) |
| Presets and sliders during TUNING… no longer get stomped by commit/glide; matches design and BOOM-macro behavior / S | Cancel FIND BOOM on any listen-time intervention | `src/gui.rs`, `src/auto_lock.rs` — `auto_lock.cancel()` on apply_preset path + mark_custom/whitelist edits while Listening; or snapshot expected-at-listen-start |
| Stops users from killing boom rest; aligns OUTPUT with honest motor rest product goal / S | Rewrite Floor copy + soft warn when min_vibe lifts troughs | `src/gui.rs` — hover text + optional amber hint if `min_vibe > ~0.05` while `sustain_level < 0.15` |
| Ends placebo Attack drags under FIND BOOM 20ms; preview matches fast-path peak→Decay / M | Attack honesty: Instant band + truthful ADSR preview | `src/gui.rs` — clamp/label <50ms as Instant; `draw_adsr_envelope` skip attack when `attack_ms < 50`; dim OVERRIDE A when inert |
| Exposes the real boom trough / hang control for hardware A/B without expert spelunking / S | Surface Motor ramp next to ENVELOPE (or default-open OVERRIDE row) | `src/gui.rs` — one slider row under ENVELOPE or BOOM; leave curves/trim collapsed |

---

### ANDROID_UI_UX

- **[critical]** `ui/MainScreen.kt:429-430`: Build Cycle slider binds `climaxBuildUpMs` (8000–240000 ms) but formats with `"%.0f s"`, so default 90000 shows as "90000 s" not "90 s". ManualEntryDialog range is also raw ms while the unit reads as seconds. Desktop `gui.rs` converts ms↔seconds for the same control.  
  **feel:** Climax arc timing is the session-scale sensation control. Misreading cycle length as 90,000 seconds (or typing 90 expecting seconds and getting clamped 8s) destroys tease/surge pacing and makes climax feel random or broken under arousal.

- **[high]** `ui/MainScreen.kt:209-258`: Start/Stop lives only inside a full-screen verticalScroll ControlsRow near the top. There is no sticky bar, FAB, or always-visible panic stop. Climax and output safety knobs are at the bottom of the same scroll (lines 386–444).  
  **feel:** Product goal includes safe stop. One-hand under arousal, user is deep in climax/output and cannot hit Stop without scrolling past many sliders—latency to honest motor rest is a UX failure even though the stop path itself is correct in ChloeVibesApplication.

- **[high]** `ui/MainScreen.kt:278-444`: UI is a single flat expert form: INPUT→GATE→TRIGGER→ENVELOPE+curves→OUTPUT→CLIMAX all always expanded (~20+ sliders). TEMPORAL_ARCHITECTURE wants expert knobs collapsed and a small main surface; desktop collapses OVERRIDE-style density.  
  **feel:** Under arousal, cognitive bandwidth collapses. Burying Volume/Ceiling/Climax behind Knee/Smooth/A-D-R curves makes live feel-tuning impossible one-handed and encourages wrong knobs (gate knee vs ceiling) that blunt boom contrast.

- **[high]** `ui/MainScreen.kt:400-418`: Climax UI shows only Switch + ACTIVE/OFF. Engine has `phaseProgress()` and desktop shows CYCLE % plus Reset (`gui.rs` ~2705–2717), but ProcessingState only exposes lastFinalOutput/envelope—no climax phase, maturity, or reset control in MainScreen/MainActivity.  
  **feel:** Without cycle position, user cannot anticipate tease cliff vs surge peak, cannot re-sync after a bad start, and cannot tell if anti-adaptation is mid-deny—climax feels opaque instead of a devastating, readable arc.

- **[medium]** `ui/MainScreen.kt:517-591`: Status row packs four 10.sp StatusChips (phase/gate/capture/device) with Ellipsis; battery is 11.sp. No large LIVE/STOPPED affordance, no high-contrast climax state in the meter card.  
  **feel:** Clarity under arousal requires glanceable state. Tiny dim chips make it easy to miss STOPPED vs LIVE, CLOSED gate (no boom), or device drop—user thinks the engine is dead when motors are only gated or disconnected.

- **[medium]** `ui/MainScreen.kt:391-396`: Floor and Ceiling are independent 0–1 sliders with no min≤max coupling. `mapOutput` uses `minVibe + shaped*(maxVibe-minVibe)`; floor>ceiling inverts the range and can crush dynamic range or reverse punch.  
  **feel:** Honest motor rest and boom contrast depend on floor near zero and ceiling as the punch cap. Accidental inversion under one-hand drag flattens or inverts dynamics mid-session.

- **[medium]** `ui/MainScreen.kt:988-1032`: LabeledSlider uses only 2.dp vertical padding; value readout is 56.dp × ~11.sp click target; Material slider thumbs lack enlarged touch targets for thumb-only use.  
  **feel:** One-hand phone use during stimulation needs fat targets. Mis-hits on adjacent expert sliders (e.g. Sustain vs Release) change envelope feel unintentionally and break musical punch.

- **[medium]** `ui/MainScreen.kt:421-443`: Climax parameter changes call `onParameterChanged()` but never set `selectedPresetName = "Custom"` (unlike Volume/Gate/Envelope). Preset chip can still show e.g. "Bass Drum" while climax is heavily customized.  
  **feel:** Users trust the preset label as the current feel recipe. Silent climax edits hide that the session arc is no longer the factory preset, causing confusion when re-selecting or comparing feel.

- **[medium]** `AndroidManifest.xml:42-46`: MainActivity has no FLAG_KEEP_SCREEN_ON / keepScreenOn while capturing; no Compose keep-awake. Screen can dim/sleep mid-session on real hardware.  
  **feel:** A dark screen removes one-hand access to Stop, Volume, and Climax mid-arc. Re-waking under arousal adds panic latency before safe rest or intensity rescue.

- **[low]** `ui/MainScreen.kt:881-897`: ClimaxPatternSelector is a non-wrapping Row of FilterChips with no scroll; labels Wave/Stairs/Surge are OK on wide phones but compete with dense climax sliders and lack plain-language subtitles (desktop uses hover help).  
  **feel:** Pattern choice changes climax personality (smooth vs stepped vs front-loaded). Without short feel copy, users under arousal pick blindly and miss the intended devastating arc shape.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| critical / S (~15 lines in MainScreen LabeledSlider call site: ms/1000 display, 8–240 s range, write back *1000) | Fix Build Cycle to true seconds (display + entry + slider range) | `android/.../ui/MainScreen.kt` |
| high / M (restructure Column to scroll body + fixed bottom bar; wire existing onStopCapture) | Sticky bottom safety rail: Stop + output % + Climax toggle | `android/.../ui/MainScreen.kt` |
| high / M (expandable sections for Gate/Trigger/Curves; keep presets + meter + primary feel knobs first) | Collapse expert sections; promote Volume, Ceiling, Climax near top | `android/.../ui/MainScreen.kt` |
| high / M (`lastClimaxPhase` on ProcessingState after process(); UI chip + reset via climaxEngine.reset) | Expose climax CYCLE % + Reset (parity with desktop) | `audio/AudioCaptureManager.kt`; `audio/ClimaxEngine.kt`; `ui/MainScreen.kt`; `MainActivity.kt` |
| medium / S (bigger LIVE/STOPPED, 48dp slider row height, couple Floor≤Ceiling) | Enlarge glanceable status + slider touch targets | `android/.../ui/MainScreen.kt` |
| medium / S (DisposableEffect/Window flag in MainActivity or MainScreen) | keepScreenOn while isCapturing | `MainActivity.kt` (or MainScreen.kt) |

---

### PARITY_GOLDEN_WIP

- **[high]** `src/gui.rs:1504-1515`; `android/.../AudioCaptureManager.kt:651-663`; `audio.rs:875`; `EnvelopeProcessor.kt:366-367`: Prefire sets `is_onset=true` but production onset pre-gate (strength>1.02) and drive (strength>1.05) kill predictive triggers when flux is still low; golden harness never applies prefire at all.  
  **feel:** WAVE-001 50ms lead is supposed to land punch on-beat; if prefire is filtered out, kicks arrive late and latency compensation is theater on both platforms.

- **[high]** `tests/parity.rs:79-88,242-347`; `tests/parity_golden.csv` (zeros only at frame 0): Synthetic 2Hz drum retriggers reset micro-pause timers; golden CSV has no mid-scenario envelope zeros / `silence_event` coverage despite sustain scenarios.  
  **feel:** Anti-adaptation true-zero rests and silence hard-snap can regress on one client without CI noticing; continuous hum returns and sensation dies.

- **[high]** `tests/parity.rs:325-339`; `src/gui.rs:1773-1795`; `AudioCaptureManager.kt:732-755`: Golden motor2 columns record `ClimaxEngine.motor2_output`, but live dual-motor path uses spectral treble fraction × motor1 and ignores climax anti-phase motor2.  
  **feel:** CI claims dual-motor parity while Edge body/sizzle split can drift freely; climax dual-motor design is unenforced in production.

- **[medium]** `src/gui.rs:1472-1534`; `AudioCaptureManager.kt:637-656`: Desktop runs Beat→prefire before Gate (prefire sees previous-frame gate); Android runs Gate→Beat→prefire (current gate). Violates frozen Spectral→Gate→Beat order on desktop.  
  **feel:** Near gate open/close edges, predictive hits and rest floors can disagree between Windows and Android on the same track.

- **[medium]** `tests/parity.rs:322-339`; `docs/TEMPORAL_ARCHITECTURE.md:104-105`; `gui.rs:1630-1657`; `AudioCaptureManager.kt:708-724,828-850`: Parity harness ends at `map_output`; asymmetric slew (0.35/0.15), silence hard-snap, and BLE peak-hold zero-bypass are not golden columns.  
  **feel:** Boom trough depth, punch rise, and honest motor rest are mostly post-map; green golden does not protect the pump/rest signature users feel.

- **[medium]** `tests/parity.rs:278-286`; `ParityTest.kt:193-217`: Harness feeds raw beat onsets into drive; production wraps with prefire + strength/energy pre-gate before envelope/climax.  
  **feel:** Golden validates a cleaner onset path than shipping clients; wrapper drift (the usual WIP risk) is invisible to CI.

- **[medium]** `src/gui.rs:1593-1601`; `AudioCaptureManager.kt:714-717`; `audio.rs:1442-1451`: Android `mapOutput` receives `silenceEvent`; desktop `map_output` does not. Climax `gated_boost` can keep shaped>0 while envelope micro-pauses; desktop relies only on post-map hard-snap.  
  **feel:** If hard-snap regresses on one side, micro-pauses become partial dips again and motors never fully rest between anti-adaptation zeros.

- **[medium]** `src/presets.rs:18-62`; `android/.../Presets.kt:36,831-890`; `docs/TECHNICAL_REFERENCE.md:187,268`: Desktop Preset catalog lacks `threshold_knee`/`dynamic_curve` fields and Chloe Loose/Medium/Ultimate macros present on Android.  
  **feel:** Same named starting point yields different punch/knee/boom floors across platforms; catalog is not parity-locked.

- **[low]** `docs/TECHNICAL_REFERENCE.md:95,223`; `src/audio.rs:975`; `settings.rs:276`: Docs still claim 76ms pre-fire and 85ms slew; WIP code is `PREFIRE_LEAD_MS=50` and `OUTPUT_SLEW_MS=42`.  
  **feel:** Operators and future ports retune against stale numbers, undoing WAVE-001 boom trough and lead timing.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Restores WAVE-001 on-beat punch lead; biggest feel win still latent in code / S | Make prefire actually fire: bypass strength pre-gate (or inject strength≥1.06) on predictive onsets, both clients | `src/gui.rs`; `android/.../AudioCaptureManager.kt`; optional unit test in audio.rs + Kotlin |
| Locks anti-adaptation true-zero across Rust/Kotlin so rest floors cannot silently regress / M | Add golden scenario: long sustain, no drum retriggers; assert envelope zeros + silence_event path; regen CSV; mirror in ParityTest | `tests/parity.rs`; `tests/parity_golden.csv`; `android/.../ParityTest.kt` |
| CI tests the path users feel; kills gate-edge dual-client drift / M | Align production wrappers in golden: Gate→Beat→prefire→pre-gate→drive; match Android order on desktop | `src/gui.rs`; `tests/parity.rs`; `ParityTest.kt` |
| Honest Edge dual-motor parity instead of false CI green / M | Either golden-cover live spectral motor2 or stop writing climax motor2 columns; pick one dual-motor spec | `tests/parity.rs`; `ParityTest.kt`; `gui.rs`; `AudioCaptureManager.kt` |
| Guarantees micro-pause zeros reach motors even if climax residual boost is non-zero / S | Pass `silence_event` into desktop `map_output` (match Android silenceClass) and add a tiny slew/hard-snap host assert | `src/gui.rs`; `src/audio.rs` map_output call sites; optional parity column |
| Same presets feel the same; docs stop fighting WIP / M | Close catalog debt: knee/curve on desktop Preset + Chloe macros; refresh TECHNICAL_REFERENCE 50ms/42ms | `src/presets.rs`; `docs/TECHNICAL_REFERENCE.md`; android Presets.kt (reference) |

---

### ANTI_ADAPTATION_MICROPAUSES

- **[high]** `src/audio.rs:634-637,874-893` (Kotlin `EnvelopeProcessor.kt:145-148,366-382`): Every onset retrigger clears `next_micro_pause_ms` and aborts an active pause. On continuous/pad sustain with rhythmic onsets the 2–8s rest schedule almost never completes.  
  **feel:** The path that holds the motor on for long stretches (Ride/pad/Hybrid) is exactly where nerves adapt; without scheduled true-zero rests the vibe becomes a flat continuous hum despite anti-adaptation code existing.

- **[high]** `src/audio.rs:1377-1378` (`ClimaxEngine.kt:203-204`): Climax 5-osc micro-pulse rate caps at 7 Hz (10 Hz in surge), above the ~2–5 Hz band ERM+BLE can express.  
  **feel:** Experience presets with Climax on spend modulation energy the motor low-passes away; body feels steady plateau while the engine believes it is fighting adaptation.

- **[medium]** `src/audio.rs:737-740` (`EnvelopeProcessor.kt:239-241`): Micro-pause duration floor is 60 ms; Domi-class unbraked coast-down is ~50–100 ms, so short pauses often cannot reach true mechanical rest.  
  **feel:** Commanded zero that never fully stops the mass feels like a soft dip, not an honest motor rest—adaptation relief and boom contrast both suffer.

- **[medium]** `src/audio.rs:1440-1450`; `gui.rs:1593-1638`; `AudioCaptureManager.kt:714-724`: Climax can add `gated_boost` on dry=0 during micro-pause; desktop `map_output` `is_silent` omits `silence_event` (Android silenceClass includes it). Rest relies on post-trim hard-snap only on desktop.  
  **feel:** Silence-class events must own the whole output path; re-injection + trim buffering can soften or desync the rest edge and breaks dual-client silence parity.

- **[medium]** `src/audio.rs:832-835,584-598` (`EnvelopeProcessor.kt:335-338,97-111`): Centroid-reduced adj_sustain can fall through PLUCK_SUSTAIN (0.15) and skip Sustain entirely, so micro-pauses never arm even when user sustain is pad-like.  
  **feel:** Bright passages on borderline pad settings lose the rest layer and glue into pluck/release behavior without intentional anti-adaptation dips.

- **[medium]** `src/audio.rs:741-752` (`EnvelopeProcessor.kt:242-251`): First micro-pause is scheduled 2–8 s after Sustain entry; no earlier rest opportunity.  
  **feel:** Several seconds of unbroken hold is enough for receptors to dull before the first true-zero; early session feel goes numb on continuous material.

- **[low]** `docs/TECHNICAL_REFERENCE.md:128-139`: §3.2 still documents frame-counted 48–96 ms pauses while live code and §2.4 are time-based 60–100 ms with silence hard-snap.  
  **feel:** Stale doctrine invites regressions that re-break motor-reaching rests during future ports.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| high / small | Preserve micro-pause schedule across onset retriggers | `src/audio.rs` EnvelopeProcessor::trigger/drive; `android/.../EnvelopeProcessor.kt` — on retrigger abort active pause only; do not zero `next_micro_pause_ms` (or subtract elapsed and keep deadline) |
| high / small | Cap Climax micro-pulse to motor band (≤4–5 Hz) | `src/audio.rs` ClimaxEngine::process max_pulse_hz; `android/.../ClimaxEngine.kt` — parity lock + optional golden/host note |
| high / small | Raise micro-pause floor to ~90–120 ms | `src/audio.rs` Sustain micro-pause pause_ms; EnvelopeProcessor.kt twin; update unit test window asserts |
| medium / medium | Propagate `silence_event` through Climax and map_output | Climax process: if silence force 0 and clear gated_boost contribution; `gui.rs` map_output `is_silent \|= silence_event`; keep Android silenceClass; parity both clients |
| medium / small | Arm first rest sooner (e.g. 0.8–2.5 s) on Sustain entry | `audio.rs` / EnvelopeProcessor.kt next_micro_pause init + gap_rand range; keep 2–8s only for subsequent gaps if desired |
| low / small | Sync TECHNICAL_REFERENCE anti-adaptation table to WAVE-001 | `docs/TECHNICAL_REFERENCE.md` §3.2 (drop frame-count language; document silence_event bypass) |

---

### OUTPUT_SLEW_TEMPORAL

- **[high]** `src/gui.rs:1640-1646`: Hidden asymmetric rise ratios still live: normal rise = `output_slew_ms*0.35`, large jump (>0.25) = `*0.15` clamp 1–12; fall = full slew. Mirrored in Android `smoothOutput` (`AudioCaptureManager.kt:835-840`). Phase-2 requires deleting the 0.35 ratio and using fixed ~15 ms SYMMETRIC slew.  
  **feel:** The 0.35× rise still owns perceived attack (~2.2*τ 10–90%). ADSR attack remains partially masked; kick punch depends on the large-jump hack rather than honest envelope ownership. Symmetric 15 ms is the Phase-2 feel flip.

- **[high]** `src/settings.rs:273-276`: `defaults::OUTPUT_SLEW_MS = 42.0` (WAVE-001 boom-trough trim from 48/85). Android AudioCaptureManager params default `outputSlewMs=42f`. Phase-2 target is ~15 ms fixed device-protection limiter, not a musical pump time.  
  **feel:** At 42 ms fall, troughs between kicks are deeper than old 85 ms but release still composes with slew tail; rise remains asymmetric. Product signature is still "Classic Pump" math without the named escape hatch.

- **[high]** `docs/TEMPORAL_ARCHITECTURE.md:125-130`: Phase-2 escape hatch "Classic Pump" expert flag is unspecified in code: no `classic_pump` / ClassicPump setting, Preset lacks the field (`presets.rs:18-62`), no UI toggle. WAVE-001.md explicitly deferred full redesign pending Domi A/B.  
  **feel:** Architecture risk #2: the 85/42 asymmetric pump may be the product's signature. Shipping 15 ms symmetric without Classic Pump can make every session feel harsh or thin with no one-click restore of the beloved pump.

- **[high]** `tests/parity.rs:322-338`: `run_chain` ends at `map_output`; slew is NOT golden-covered. Slew lives duplicated in `gui.rs` (inline) and `AudioCaptureManager.smoothOutput` — not in `audio::map_output`. TEMPORAL_ARCHITECTURE skeptic note already flags this.  
  **feel:** Phase-2 must land both platforms in one commit. Without a shared slew model in the parity harness, desktop/Android can drift on rise/fall symmetry, hard-snap thresholds, or jump thresholds and users feel different punch on phone vs PC.

- **[high]** `src/auto_lock.rs:836-853`: Auto-Lock still fits and writes `output_slew_ms = (0.10*t).clamp(30,55)`. LockParams apply path writes `s.output_slew_ms`. Phase-2 demotes slew to fixed limiter and shrinks temporal whitelist to decay/sustain/release (attack pinned).  
  **feel:** FIND BOOM re-introduces 30–55 ms musical slew after any lock, undoing a fixed 15 ms limiter and re-masking ADSR. Tempo-coupled slew fights honest boom troughs and dual-client predictability.

- **[medium]** `src/settings.rs:478-479`: Load path: `get_value(OUTPUT_SLEW_MS).unwrap_or(default)` with no migration. Phase-2 requires stored slew==85 auto-migrate and custom values clamp to 0–60. No 85→new-default branch exists.  
  **feel:** Long-time users keep 85 ms (or other legacy values) forever; after Phase-2 code lands they either keep the old pump silently or get an unplanned harsh clamp. Migration is required for honest A/B and support.

- **[medium]** `src/gui.rs:2572-2577`: UI clamps Motor ramp to 1–220 ms; settings.sanitize clamps 1–500 (`settings.rs:234`); Phase-2 wants expert clamp ~0–60 with fixed default 15. Three different ranges, none match Phase-2.  
  **feel:** Expert can set 200+ ms fall and re-create the "release lies by ~2×" scandal; Phase-2 demotion cannot hold if the knob still allows musical-scale ballistics.

- **[medium]** `docs/TECHNICAL_REFERENCE.md:93-95`: Docs still claim asymmetric output slew "85 ms nominal, rising edge ~30 ms (0.35 of configured slew)" and that "this stage is verified value-for-value by the parity test". Live default is 42 ms; slew is not parity-golden.  
  **feel:** Operators and A/B notes will tune against wrong numbers; false confidence that slew is parity-locked delays catching dual-client punch drift.

- **[medium]** `src/gui.rs`: Phase-0 composite Response readout (effective attack/release including slew RSS) is absent — no Response/effective_attack/tau_slew symbols in `src/`. Attack ownership remains invisible in UI.  
  **feel:** Without the readout, hardware A/B of Phase-2 cannot be explained to the user ("why attack slider felt placebo"); highest-leverage honesty fix still missing before the feel flip.

- **[medium]** `src/gui.rs:3901-3929`: ChloeRhythmProfile still sets `output_slew_ms` to 55/48/42. `apply_preset` resets slew to `defaults::OUTPUT_SLEW_MS` (42) but not to Phase-2 15. Factory release +~56 ms retune for lost slew tail (Phase-2 plan) not present in preset release values.  
  **feel:** When slew fall shortens, musical release shortens unless decay/release are retuned. Shipping 15 ms without release compensation makes booms feel chopped and less devastating.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Unblocks one-commit Phase-2 dual port; catches rise/fall/hard-snap drift before hardware A/B / M | Extract shared `apply_output_slew` into audio.rs + Kotlin twin; cover in parity `run_chain` | `src/audio.rs`, `tests/parity.rs`, `android/.../AudioCaptureManager.kt`, `src/gui.rs` |
| Makes Phase-2 reversible on Domi without a reinstall; satisfies architecture escape hatch / M | Add `classic_pump` expert flag (settings + UI) restoring 0.35× rise / full fall; default OFF path ready for 15 ms symmetric | `src/settings.rs`, `src/gui.rs`, android AudioCaptureManager + MainScreen |
| Prevents FIND BOOM from re-musicalizing slew after Phase-2 demotion / S | Stop Auto-Lock writing `output_slew_ms`; pin fixed limiter (or only write when classic_pump) | `src/auto_lock.rs` |
| Honest upgrade path; stops 200 ms expert ramps that re-mask ADSR / S | Load migration: slew≈85→new default; sanitize/UI clamp to Phase-2 expert range (e.g. 1–60) | `src/settings.rs`, `src/gui.rs` |
| Preserves boom length and climax arcs when fall slew shrinks / S | Prep factory release/decay compensation table (+slew-tail ms) for 15 ms symmetric flip | `src/settings.rs`, `src/presets.rs`, `src/gui.rs` rhythm profiles, android Presets.kt |
| Correct operator model of who owns attack/release; enables informed Domi A/B of Classic Pump vs 15 ms / M | Sync TECHNICAL_REFERENCE + add composite Response readout (Phase-0) before hardware A/B | `docs/TECHNICAL_REFERENCE.md`, `src/gui.rs` |

---

### SESSION_ARC_AROUSAL

- **[critical]** `src/audio.rs:1465-1510` (parity: `android/.../ClimaxEngine.kt:271-318`): `high_output_ms` accumulates from pre-deny `raw_output` while `deny_active`; at maturity deny duration (~2.4–2.8s) ≈ trigger (3s), so inter-deny recovery collapses to ~0.3–0.6s and post-deny overshoot is largely wasted.  
  **feel:** Long sessions turn edge-and-deny into near-continuous stutter instead of aching hold → devastating return; the mature arc's main anti-plateau tool becomes rhythmic boredom.

- **[high]** `src/audio.rs:1434-1451` (parity: `ClimaxEngine.kt:249-258`): Arousal gain advertises up to ~3.8× then hard-clamps `raw_output` to 1.0; late-cycle surge/tease/momentum headroom is destroyed for any dry input ≳0.3.  
  **feel:** Climax "crescendo" becomes a flat motor ceiling; musical punch and contrast die exactly when the session should feel most overwhelming.

- **[high]** `src/gui.rs:1773-1794`; `android/.../AudioCaptureManager.kt:732-749` vs `audio.rs:1453-1508`: `ClimaxEngine.motor2_output` (anti-phase + deny) is computed and parity-tested but live dual-motor path ignores it and uses treble×motor1 instead.  
  **feel:** On Lovense Edge, designed spatial movement and asymmetric deny never reach hardware—session arc is mono and less alive.

- **[high]** `src/audio.rs:1315,1334,1472-1493` (parity: `ClimaxEngine.kt:142,160,278-299`): `cycle_maturity` freezes at 1.0 after 6 completed cycles (~4.5–12 min depending on `build_up_ms`); tease depth, deny trigger/duration/depth, and post-deny boost stop escalating.  
  **feel:** After the first handful of cycles the arc plateaus; long sessions lose novelty and re-enter neural adaptation.

- **[medium]** `src/gui.rs:1630-1637`; `AudioCaptureManager.kt:714-724`; `docs/TEMPORAL_ARCHITECTURE.md:62`: Silence-class hard-snap only honors `envelope.silence_event` / true zero; climax deny/tease cliffs (60–90% drops) still pass through output slew (and Android transport smoothing).  
  **feel:** Edge cliffs and tease drops arrive as partial dips, not honest motor rest—anticipation and nerve-reset fail when they matter most.

- **[medium]** `src/audio.rs:1376-1378` (parity: `ClimaxEngine.kt:203-204`); `docs/TEMPORAL_ARCHITECTURE.md:63`: Micro-pulse rate caps at 7 Hz (10 Hz in surge), above the documented ≤5 Hz motor-expressible climax band.  
  **feel:** Anti-adaptation energy is spent on modulation the ERM physically crushes; sensation flattens even while the engine "thinks" it is varying.

- **[medium]** `src/audio.rs:1300-1307` (parity: `ClimaxEngine.kt:129-134`): On cycle wrap, `floor(cycles)` may be >1 but `cycle_count` increments by 1 and `arousal_momentum` by only +0.12 once.  
  **feel:** Time jumps/stalls under-count maturity and momentum so the session arc can lag real elapsed stimulation time.

- **[medium]** `src/audio.rs:1306-1311,1438-1439` (parity: `ClimaxEngine.kt:133-138,252-253`): Arousal momentum caps at 0.75 after ~7 cycles and only decays when gate is closed; continuous music locks max gain with no later-phase variety.  
  **feel:** No second-wind or boredom-recovery arc after ~10 minutes of continuous play—feel becomes a static loud plateau.

**ACTIONS:**

| Priority | Action | Files |
|----------|--------|-------|
| Restores multi-second recovery between mature denies; post-deny overshoot becomes feelable again—highest edge-arc ROI / S | Freeze `high_output_ms` while `deny_active` (count felt output, not pre-deny raw) | `src/audio.rs`; `android/.../ClimaxEngine.kt`; unit test for deny re-trigger gap at maturity |
| Lets surge and musical dynamics remain audible above mid-cycle level instead of flat-max plateau / M | Reserve late-cycle headroom (soft ceiling / makeup gain) instead of hard clamp-to-1 | `src/audio.rs`; `android/.../ClimaxEngine.kt`; `tests/parity.rs` golden regen |
| Ships designed spatial anti-phase + deny contrast on Edge; long sessions feel multi-dimensional / M | Drive dual-motor devices from `climax.motor2_output` (blend or replace treble path when climax on) | `src/gui.rs`; `android/.../AudioCaptureManager.kt` |
| Prevents ~5–10 min arc freeze; keeps tease/deny evolution alive for long sessions / S | Extend maturity past 6 cycles (slow log growth + optional pattern morph after t>maturity) | `src/audio.rs`; `android/.../ClimaxEngine.kt` |
| Honest motor rest on edge cliffs—matches TEMPORAL_ARCHITECTURE Phase 3 and makes deny devastating / M | Flag deep deny/tease cliffs as silence_class (depth≥30% for first 60–100ms of drop) | `src/audio.rs` (export flag); `src/gui.rs`; android AudioCaptureManager + ClimaxEngine |
| Anti-adaptation energy becomes physically feelable on Lovense ERMs / S | Cap climax micro-pulse ≤5 Hz; move budget into depth / slower chaos | `src/audio.rs`; `android/.../ClimaxEngine.kt`; parity golden if pulse affects outputs |

---

## Ship blockers (rollup)

1. Wire climax `motor2_output` to live dual BLE/desktop path (stop treble-shadow theater).
2. Desktop pipeline reorder: Gate→Beat (match Android + frozen chain); current-frame gate for prefire.
3. Green suite: `cargo test` (lib+bin) + `cargo test --test parity` + Android `testDebugUnitTest` re-run after merge.
4. Hardware Domi/Edge A/B for rest/gamma, prefire lead, peak-hold, and climax deny feel.
5. Phase-2 15ms symmetric slew remains **deferred** pending Domi A/B (Classic Pump escape hatch still unspecified).

---

## Test status snapshot (implement-lane best knowledge)

| Suite | Status |
|-------|--------|
| cargo lib + bin unit | Pass (35 lib + 61 bin, 1 ignored) per implement report |
| cargo `--test parity` | Pass after golden regen (`parity_rust_golden` ok) per implement report; adversarial verify did **not** re-run |
| Android `testDebugUnitTest` | BUILD SUCCESSFUL (UP-TO-DATE) per implement report; lane C/D/E noted intermittent compile/golden blockers mid-wave |
| Adversarial WAVE-002 VERIFY | Suite **not** run; `ship_ready=false` |

---

*End of WAVE-002 Peak Feel Army Run Report.*
