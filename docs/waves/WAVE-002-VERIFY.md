# WAVE-002 VERIFY — adversarial feel review

**Status:** VERIFY (adversarial)  
**Date:** 2026-08-08  
**Repo:** `C:\Users\coldb\Chloe-Vibes`  
**Scope:** Working-tree diff vs `master` (`365b53d` + dirty overlay) against `WAVE-002-PEAK-FEEL.md` claims  
**Reviewer posture:** Challenge every “feel improved” claim; demand path:line / symbol evidence; no credit for theater  

**Suite status (this review):**  
- `cargo test` / `cargo test --test parity` / `cargo clippy`: **NOT RUN** → `cargo_ok=false`  
- `gradlew testDebugUnitTest`: **NOT RUN** → `android_tests_ok=false`  
- Hardware Domi / Edge A/B: **NOT RUN**  

---

## Executive verdict

| Field | Value |
|-------|-------|
| **ship_ready** | **false** |
| **cargo_ok** | **false** (tests not executed by reviewer) |
| **android_tests_ok** | **false** (unknown / not run) |
| **parity_ok** | **false** (structural host-path mismatches + suite not green under this review) |

**One-line (post-P0 fix pass, same day):** Army WIP + integrator P0 pass. Dual-motor climax is now wired when climax is ON; desktop pipeline is Gate→Beat with current-frame prefire; map_output takes silence-class; onset pre-gate is 1.05; gate-edge→onset velocity upgrade; post-deny soft ceiling 1.12. Suite green after golden regen. Still needs **hardware Domi/Edge A/B** before ship; Phase-2 15ms slew still deferred.

---

## Pass / fail by WAVE-002 lane

### M1 — Prefire actually fires — **PARTIAL (P1 residual)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Synthetic strength floor so drive (≥1.05) accepts prefire | **PASS (code)** | `BeatDetector::take_prefire` → `recent_onset_strength.max(1.15).min(1.35)` in `src/audio.rs`; Kotlin twin `takePrefire` in `BeatDetector.kt` |
| One-shot latch per predicted beat | **PASS (code)** | Prediction advanced by `tempo_interval_ms` on consume (Rust + Kotlin) |
| Tempo conf decay + hard clear `<0.05` | **PASS (code)** | `decay_tempo_confidence` / `clear_tempo_lock` both clients |
| Lead 50 ms | **PASS (code)** | `PREFIRE_LEAD_MS = 50` |
| Desktop reorder **Gate → Beat → prefire** (current-frame gate) | **FAIL** | `src/gui.rs` still runs Beat+prefire **before** Gate (`// 4. Beat` then `// 5. Gate`). Prefire gates on **stale** `self.gate_is_open` from prior frame. Android correctly does Gate then Beat (`AudioCaptureManager.kt` steps 3–4). **Parity break.** |
| Single onset threshold 1.05 (kill 1.02–1.05 dead band) | **FAIL / residual** | Host pre-gate still `onset_strength > 1.02` (`gui.rs` `onset_ok`; Android `onsetStr <= 1.02f`). Drive still requires `> 1.05`. Prefire bypasses via 1.15 floor; **real weak onsets still die in the dead band.** |

**Feel challenge:** “On-beat prefire” is **plausible** when tempo is locked and gate was already open last frame, but desktop can still miss the first kick of a phrase (gate opens same frame as prefire window) and can still ghost relative to Android. Do **not** ship as “latency fixed dual-platform.”

---

### M2 — Motor-honest boom silence + velocity punch — **PARTIAL (P1 residual)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| `silence_event` on pluck/boom true-rest + micro-pause | **PASS (code)** | Envelope Idle/Release/micro-pause paths set `silence_event` (Rust + Kotlin) |
| Stop double-scale-then-clip; overshoot in `attack_target` | **PASS (code)** | `vel_for_mag = velocity.min(1.0)`; overshoot `(1+0.30*(v-1)).min(1.28)`; process cap **1.20** |
| Gate-edge + imminent onset merge (upgrade velocity inside retrigger) | **FAIL** | `EnvelopeProcessor::drive`: `gate_just_opened \|\| is_onset_trigger` with gate-only path `velocity = 1.0`; `min_retrigger_ms=20` still **blocks** a real onset that arrives inside the window. No merge/upgrade path. |
| Unify pluck vs organ (`continuous_hold` uses centroid-adjusted sustain) | **PASS (code)** | `adj_sustain_level > PLUCK_SUSTAIN` both clients — Domi hang regression risk **reduced** vs re-arm on raw sustain |

**Feel challenge:** Punch headroom math is better on paper; **gate-edge thump still steals the real onset** on tight kicks. Boom silence depends on M3 host wiring (mostly OK) + BLE peak-hold (improved).

---

### M3 — Silence-class owns full output path — **PARTIAL (P0/P1 residual on desktop map)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Climax dry-zero / deep deny → `silence_event`, clear residual boost | **PASS (code)** | `ClimaxEngine` dry≤0.001 and deny_envelope≥0.30 → silence + motor2 0 |
| Android `mapOutput` + slew hard-snap on silence-class | **PASS (code)** | `AudioCaptureManager`: `silenceClass = isSilent \|\| envelope.silenceEvent \|\| climax.silenceEvent`; map + force 0 |
| Desktop `map_output` / host `is_silent \|= silence_event` | **PARTIAL** | Desktop still calls `map_output(..., is_silent)` with **energy/gate/Idle only** (`gui.rs` ~1613–1621). Hard-snap **after** map uses `envelope.silence_event \|\| climax.silence_event` (~1654–1658). Motor1 usually zeros, but **min_vibe floor can re-enter map before snap**; trim delay ring can still hold pre-silence samples. |
| BLE peak-hold ~20–25 ms + large down-step / zero bypass | **PASS (code)** | `peakHoldMs=22`, `peakHoldDropBypassSteps=3`, level≤0 clears (`BleDeviceManager.kt`) |

**Feel challenge:** “True motor rest” is **much more credible on Android** than WAVE-001. Desktop is close via post-map hard-snap but **not** the frozen silence-class contract through `map_output`.

---

### M4 — Edge & Deny product path + devastating arc — **PARTIAL (P0 dual-motor; P1 post-deny)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Named “Edge & Deny” has `climax_enabled=true` + deep edge params | **PASS (code)** | `src/presets.rs` + `Presets.kt`: climax ON, tease_drop 0.72, Stairs, min_vibe 0 |
| Tease THEN surge non-overlapping | **PASS (code)** | `surge_start=0.88`, tease window before surge (Rust + Kotlin) |
| Post-deny return punch survives 1.0 | **FAIL (claim overstated)** | Exit sets `onset_boost += post_deny` up to 0.85, but `raw_output` still `.clamp(0.0, 1.0)` / `.coerceIn(0f, 1f)`. On loud dry plateaus boost **adds nothing felt**. No temporary headroom / soft ceiling. |
| Deep deny → silence-class near-rest | **PASS (code)** | deny_envelope ≥ 0.30 → return 0 + silence_event |
| Freeze `high_output_ms` while deny active | **PASS (code)** | Guarded by `!deny_active` |
| Wire `motor2_output` to dual devices when climax ON | **FAIL (P0 product lie)** | Live hosts still derive motor2 from **treble spectral shadow** (`gui.rs` ~1800–1820; `AudioCaptureManager.kt` ~769–790). `ClimaxEngine.motor2_output` is computed and golden-tested but **not sent to the toy**. Edge spatial climax claim is theater. |
| Cap micro-pulse ≤5 Hz | **PASS (code)** | `max_pulse_hz = 5.0` / sub-harmonic capped |
| Multi-wrap `cycle_count` + maturity past 6 | **PASS (code)** | floor(cycles) advance + log maturity |

**Feel challenge:** Edge & Deny is **no longer a no-op preset** — that alone is a real product fix. “Devastating arc / dual-motor climax / post-deny punch on already-loud body” is **not fully earned**.

---

### M5 — Android audio-path dead-man + durable stop — **PASS (code review; tests not run)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| 2s dead-man on processing heartbeat | **PASS (code)** | `AudioPipelineWatchdog.DEFAULT_TIMEOUT_MS=2000`; poll from `ChloeVibesApplication` |
| Lease-fence motor writes | **PASS (code)** | `outputLock` + `OutputOwner.Audio` gate on `onOutputUpdate` / dual |
| `stopMotors` before disconnect | **PASS (code)** | `BleDeviceManager.disconnect` try-stop; Application emergency paths |
| Fail-closed processing loop | **PASS (code)** | `onPipelineFailClosed` → dead-man trip |
| Sticky Stop all + UI banner | **PASS (code)** | `MainScreen` safety rail + `emergencyStopAll` |
| Unit tests patterned on companion dead-man | **PRESENT / unrun** | `AudioPipelineWatchdogTest.kt` |

**Residual risks (watchdog false positives):**  
- Heartbeat only advances on **successful** frames; sustained capture starvation (Visualizer hang without exception path) correctly trips — good.  
- Aggressive trip if main thread starves the 250 ms poll while processing is fine is **unlikely** (poll ages stamp, not UI).  
- Companion path uses separate 1.75s lease — intentional; do not claim one dead-man covers Finally bridge.  
- `stopMotors` queues stop into `pendingCommand` LWW — improved, but radio still single-flight; not a verified “device is still” without hardware.

---

### M6 — Anti-adaptation rests that complete — **PASS (code)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Retrigger aborts active pause only; keep schedule | **PASS** | `micro_pause_until=0` on trigger; `next_micro_pause` not zeroed |
| Pause floor ~90–120 ms time-based | **PASS** | Replaces frame-count 48–96 ms |
| First rest sooner 0.8–2.5 s | **PASS** | Arm on Sustain entry |

**Feel challenge:** On Domi, 90–120 ms is closer to coast-down honesty than 3–6 frames; still **no hardware proof**. Continuous pads should finally get zeros; boom paths still depend on pluck sustain + silence-class (M2/M3).

---

### M7 — Default / boom preset honesty — **PASS (code)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Club 125 slew 42 ms aligned with Bass Drum | **PASS** | presets + `apply_chloe_rhythm_profile` |
| Deep 90 / Club 125 / Hard 140 in factory catalog | **PASS** | Both clients; Android renames Chloe Loose/Medium/Ultimate |
| `output_slew_ms` / knee / curve on apply | **PASS** | `Settings::apply_preset` writes preset knee/curve/slew |
| Missing settings keys → Hybrid + LowPass | **PASS** | `settings.rs` load fallbacks |
| Edge & Deny climax ON | **PASS** | (see M4) |

**Residual:** Saved custom “Chloe *” names migrate on find/startup; users with hand-tuned clones of old macros may still diverge. Unit tests in `presets.rs` assert catalog locks — **not executed here**.

---

### M8 — UX under arousal — **PARTIAL (Android mostly PASS; desktop FAIL)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Android Build Cycle true seconds 8–240, write ×1000 | **PASS** | `MainScreen.kt` display `/1000`, write `*1000` |
| Sticky Stop + output % + Climax toggle | **PASS** | Safety rail |
| Climax CYCLE % + Reset | **PASS** | `lastClimaxPhase` / `onClimaxReset` |
| `keepScreenOn` while capturing | **PASS** | `MainActivity.applyKeepScreenOn` |
| Floor ≤ Ceiling coupling | **PASS** | Slider coerce |
| Desktop ENERGY GATE plot gate-domain (not post-volume) | **FAIL** | No working-tree fix in `gui.rs` for plot domain |
| Clear `last_report` on auto_lock cancel; report only when Locked | **FAIL** | Not in diff |
| Cancel FIND BOOM on preset/slider intervention | **FAIL** | Not in diff |
| Floor hover / soft warn | **FAIL** | Not in diff |

**Feel challenge:** Android arousal UX is a real safety/readability win. Desktop boom honesty UX claims from M8 are **unshipped**.

---

### M9 — BLE DeviceType + curve session integrity — **PARTIAL (P1)**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| MotorFeel lockstep with desktop floors/gammas | **PASS (code)** | Wand 0.026/1.68, Compact 0.032/1.40, DualBody 0.026/1.55, Generic 0.024/1.48 — matches `MotorKind` in `gui.rs` |
| Reset dual + feel on connect/disconnect | **PASS (code)** | `connectInternal` / `disconnect` |
| Provisional curves from advertised name / LVS hint | **PASS (code)** | `fromAdvertisedName` + `modelHint` |
| Nora A/C → DualBody | **PASS (code)** | `fromDeviceTypeId` includes A, C |
| Priority control queue + retry until DeviceType | **FAIL / incomplete** | Single `pendingCommand` LWW; DeviceType request is not a fenced priority lane vs Vibrate. Race: early Vibrate can still win the queue before type lands (provisional feel mitigates **curves**, not dual-motor bit). |

**Feel challenge:** Raising Domi rest floor + gamma **should** reduce hang vs residual softs; it also **thins soft dynamics** — possible “feel worse on quiet passages” regression without Domi A/B. Do not market as pure win.

---

### M10 — Parity harness protects feel contract — **FAIL under this review**

| Claim | Verdict | Evidence |
|-------|---------|----------|
| Golden regenerated | **PRESENT** | `tests/parity_golden.csv` large rewrite; `.new` untracked |
| Prefire → envelope dual tests | **PARTIAL** | Rust unit tests in `audio.rs`; Android suite not verified run |
| Dual-motor golden = **live** path | **FAIL** | Parity still samples `climax.motor2_output` (`tests/parity.rs` / `ParityTest.kt`) while **hosts ignore it** when climax is on — CI can be green while Edge body is mono+treble |
| Suite green | **UNKNOWN** | Reviewer did not run cargo/gradle |

---

## Cross-cutting adversarial findings

### P0 — must not claim ship

1. **Climax dual-motor is dead UI / dead path for the body**  
   Engine writes `motor2_output`; BLE dual path still gets treble×motor1. Edge & Deny “spatial contrast” is a **ghost feature**.

2. **Desktop pipeline order still violates frozen chain**  
   Frozen: Spectral → **Gate → Beat** → Envelope → Climax → Output. Desktop: Beat/prefire before Gate. Prefire honesty and dual-client lockstep are compromised.

3. **Tests not run in this verify**  
   `cargo_ok=false`, `android_tests_ok=false`. Golden churn without a green suite is not a landing.

### P1 — feel regressions / incomplete honesty

4. **Post-deny punch claim is marketing** while `raw_output` clamps to 1.0.  
5. **Gate-edge velocity=1.0 + 20 ms retrigger** still blocks real onset overshoot (M2 merge missing).  
6. **Host 1.02 vs drive 1.05 dead band** remains (prefire papered over only).  
7. **Desktop map_output omits silence_event** (hard-snap mostly saves motor1).  
8. **DeviceType not priority-queued** — dual flag can lag; first hits may be mono.  
9. **Desktop M8 boom honesty UX** not implemented.  
10. **Wand rest/gamma upshift** may reduce Domi hang **or** mute musical softs — hardware-unknown.

### Domi hang specifically

- **Mitigations present:** higher rest floor + gamma; continuous_hold uses adj sustain; silence hard-snap; peak-hold shortened; stop preempts pending vibrate; disconnect zeros.  
- **Regression vectors:** more aggressive true-zero + shorter peak-hold can feel “clicky”; higher gamma can make mid-level body thinner; gate-edge thump without onset merge can feel like double-hit or dull first hit.  
- **Verdict:** Hang risk **directionally improved in code**; **not closed** without Domi A/B.

### Ghost pre-fire

- Latch + conf decay + 50 ms lead + recency: **real improvement** vs WAVE-001 theater.  
- Residual ghosts: desktop stale gate; locked tempo on residual gate-open noise; no device-class BLE lead table (50 ms may be early on slow Android radio).

### Climax adapts to mush

- Experience presets lowered min_vibe / sustain and capped pulse_depth — **good anti-mush intent**.  
- Continuous high `arousal_gain` + chaos + breathing still push long sessions toward dense plateaus; deny silence-class is the main rescue. Maturity log growth past 6 **escalates** rather than rests more — adaptation risk remains by design.

### BLE spam

- Steady interval 28 ms (~36 Hz); stop interval 12 ms — **stops may chatter faster** during rest storms (micro-pauses). Not classic intensity spam; watch for radio congestion on dual-motor + dither.  
- Logging now includes stop TX — debug noise only.

### Safety regressions

- Android primary path **much safer** (dead-man, fence, emergency stop, Application ownership).  
- Residual: Activity destroy no longer always stops if companion session alive (`disconnectIfNoCompanionSession`) — correct for bridge, but operator must understand companion lease.  
- Signature-permission bridge service exported — pre-existing pattern; not expanded attack surface in a feel-breaking way.

### Dead UI

- Climax CYCLE % / Reset / Stop all: **wired** on Android.  
- Climax dual-motor spatial feel: **dead** (engine-only).  
- Desktop FIND BOOM / ENERGY GATE honesty items: **unchanged** (stale claims if WAVE doc implies done).

---

## Lane scoreboard

| Lane | Result | Severity if fail |
|------|--------|------------------|
| M1 Prefire | **PARTIAL** | P1 parity / first-kick |
| M2 Silence + velocity | **PARTIAL** | P1 gate-merge |
| M3 Silence-class path | **PARTIAL** | P1 desktop map |
| M4 Edge & Deny arc | **PARTIAL** | **P0** dual-motor live path |
| M5 Android dead-man | **PASS*** | *code only; suite unrun |
| M6 Anti-adaptation rests | **PASS*** | *code only |
| M7 Preset honesty | **PASS*** | *code only |
| M8 UX arousal | **PARTIAL** | P1 desktop |
| M9 BLE DeviceType / curves | **PARTIAL** | P1 type race |
| M10 Parity harness | **FAIL** | P0 process |

\*PASS means code review supports the claim; not hardware- or suite-proven.

---

## Residual risks (ship blockers vs accept-with-eyes-open)

### Blockers for `ship_ready=true`

1. Wire **live** dual-motor: when climax enabled and device dual → send `climax.motor2_output` (both hosts); golden must match that path.  
2. Desktop **Gate → Beat → prefire** reorder with current-frame gate.  
3. Run and land green: `cargo test`, `cargo test --test parity`, `cargo clippy -- -D warnings`, `gradlew testDebugUnitTest`.  
4. Either implement post-deny headroom past 1.0 **or** strike the claim from product copy.  
5. Hardware A/B checklist from WAVE-002 (Domi troughs, Edge deny/motor2, Android hang→stop ≤2s).

### Accept-with-eyes-open (if forced interim)

- Prefire strength+latch (with desktop gate caveat).  
- Edge & Deny climax ON (mono path).  
- Android dead-man + Stop all.  
- Boom catalog rename + slew 42.  
- Silence hard-snap + shorter peak-hold (Domi click risk).

---

## What improved (credit where earned — still not ship)

- Prefire is no longer pure UI theater: strength floor + one-shot latch + conf decay.  
- Envelope silence_event + time-based micro-pauses 90–120 ms + schedule-survives-retrigger.  
- Velocity overshoot no longer double-scaled into a 1.0 brick.  
- Edge & Deny / experience presets actually enable climax with lower floors.  
- Android safety architecture (Application owner, watchdog, output fence) is the largest non-feel reliability win.  
- BLE stop preemption + peak-hold honesty + MotorFeel lockstep.  
- Preset catalog boom names + apply_preset slew/knee honesty.

---

## Recommended next commits (ordered)

1. Host motor2: climax ON → `motor2_output`, climax OFF → treble shadow (document in parity).  
2. Desktop pipeline reorder Gate→Beat→prefire.  
3. Gate-edge + onset merge inside `min_retrigger_ms`.  
4. Unify host pre-gate to 1.05 (or drive to 1.02 — pick one).  
5. Desktop `map_output(..., silence_class)`.  
6. Post-deny temporary headroom **or** doc fix.  
7. DeviceType priority/retry until applied.  
8. Desktop M8 honesty if still in wave scope.  
9. Full suite + Domi/Edge A/B before any ship tag.

---

## Close criteria (observer)

- [ ] All P0 items closed with path:line evidence  
- [ ] `cargo_ok=true` and `android_tests_ok=true` under a real run  
- [ ] `parity_ok=true` only if live dual-motor path matches golden  
- [ ] Domi: true rest between kicks; no residual level-1 hang  
- [ ] Edge: deny stillness + **felt** motor2 contrast with climax ON  
- [ ] Android: kill processing → motors stop ≤2s; Stop all verified zero  

**Result:** WAVE-002 remains **not ship-ready**. Treat working tree as **strong WIP**, not a feel-complete landing.
