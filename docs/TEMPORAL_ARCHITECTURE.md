# Temporal Architecture — Research Report & Redesign Program

Deep-research synthesis (2026-07-03): how input smoothing, the ADSR, and the
output slew should compose. Produced by a grounded multi-agent study —
code-exact stage mapping, three research lanes (pro-audio dynamics
architecture, haptic actuator physics + vibrotactile perception, shipped
audio-to-haptics systems), a design synthesis, and an adversarial DSP review
that verified every claim against this repo. Full JSON in the session
workflow transcript; this document is the curated, skeptic-corrected version.

## The diagnosis in one sentence

Three UI panels claim to do attack/release; one hidden ratio actually does.

The chain violates the single-ownership rule every surveyed pro system obeys
(one detector smoother + one shaper — Giannoulis JAES 2012; Meta/Lofelt,
Apple Core Haptics, Immersion all ship ONE envelope plus a transient
overlay). ChloeVibes has SIX live serial ballistics stages in the motor path
plus TWO dead ones.

## Verified scandals (all confirmed against code by the adversarial pass)

1. **Input Rise/Fall is dead in the motor path.** The smoothed value feeds
   only the UI energy meter and motor2's is_silent check. The gate and the
   envelope receive UNSMOOTHED energy. Android has no such stage at all.
   Auto-Lock was "tuning" two parameters that do nothing.
2. **The real attack is the output slew's hidden 0.35 rise ratio** (~65ms
   10-90% rise at the 85ms default). It masks everything upstream under
   ~30ms — which is why the attack slider reads as placebo (the <50ms
   fast path made it instant anyway).
3. **Release lies by ~2x.** The 320ms knob composes with a hidden centroid
   stretch (x1.0-1.4), the slew fall tail (~210ms residual), Android
   peak-hold + BLE interval, and 50-100ms of unbraked ERM spin-down:
   measured 480-680ms to still.
4. **The anti-adaptation micro-pauses never reach the motor.** "True zero,
   motor must stop" (audio.rs) is pushed through the slew's fall smoothing
   and Android's 55ms peak-hold; it arrives as a partial dip. Additionally
   the pause length is counted in CALLS, not ms — at the measured ~240fps
   repaint the "48-96ms" pause is actually 12-25ms. (Same defect makes the
   hidden sustain smoother ~4x faster than designed.)
5. **Tempo confidence never decays.** It is only written when onsets arrive,
   so it freezes at its last value forever; the 76ms pre-fire can ghost-fire
   on stale predictions whenever the gate is open.
6. Composition math (Wallman): cascaded smoothers compose by
   root-sum-of-squares of their rise times; a stage under half the dominant
   stage's rise adds <12% and is imperceptible. Amplitude JND ~14-20%
   (Weber); attack asynchrony detectable ~54ms; ERM motors are themselves a
   ~50-100ms low-pass with no braking over BLE; modulation above ~2-5Hz is
   physically crushed by the motor.

## Role assignment (one job per stage)

| Stage | Job | User-facing? |
|---|---|---|
| Capture / FFT window (42.7ms Hann) | THE detector smoother — input smoothing budget is spent here | No (fixed) |
| NEW squelch (pre-normalization floor) | The silence authority | Advanced only |
| BeatDetector + pre-fire | Event timing + latency cancellation (a lookahead limiter); gains confidence decay | No (fixed) |
| Gate | Binary audibility decision; instant open stays; user faces LEVELS (threshold, auto amount), not times | Levels only |
| Input rise/fall | DELETED from UI + Auto-Lock; frozen constants for the meter + motor2 silence check | No |
| **ADSR** | **The single owner of temporal feel**; attack 0.5-50ms labeled "Instant — motor spin-up is the ramp"; centroid stretch made visible | **Yes — the one panel** |
| Sustain magnitude smoother | Fixed loudness-jitter filter; must be made time-based (currently per-call) | No |
| Micro-pauses / climax drops | "Silence-class events": depth >=30% for 60-100ms, flagged to BYPASS slew fall + Android peak-hold (the stopMotors() path) | No |
| Climax | Session-scale arc; rates capped to the motor's expressible band (<=5Hz) | Yes (as today) |
| Output slew | DEMOTED to device-protection rate limiter: fixed ~15ms SYMMETRIC (0.35 ratio deleted), expert page only | Expert only |
| Device task / BLE / peak-hold | Transport codec; physics; peak-hold gains the silence-event bypass | No |

Net user surface: gate threshold, ONE ADSR panel with a live composite
"Response" readout, output range/gain, Climax, trim, and at most one "Pump"
macro (transparently scales decay+release on the displayed values).

## The composite model (what the readout computes)

Convention: alpha = 1-exp(-dt/tau); 10-90% rise = 2.2*tau. Pure lags add
linearly; cascaded smoothers compose by RSS of rise times.

- **Effective attack** ~= [capture + energy-build + frame/2 + tick/2 + BLE
  spin] + sqrt((2.2*tau_attack)^2 + (2.2*tau_slewRise)^2).
  Today at defaults: ~52 + 65 ~= **115ms** — matching the code's own
  "85-115ms late" admission. After redesign: ~85ms command-side, ~55-70ms
  with the pre-fire retuned to the fixed-lag sum (~50ms instead of 76).
- **Effective release** ~= FFT drain + frame + gate close +
  release_ms*(1+0.4*(1-cn)) + 2.5*tau_slewFall + tick [+ peak-hold + BLE on
  Android] + ERM spin-down. Today: **480-680ms** for the 320ms knob.

The readout is pure display math in gui.rs — zero engine change, zero parity
cost — and is the highest-leverage fix for the "confusing" complaint.

## Auto-Lock: from nine heuristics to one composite solver

Solve the series system, subtract the fixed constants (the Giannoulis
auto-ballistics pattern): pick ONE end-to-end response target from the
folded beat, then derive the single remaining writable stage per direction.
Whitelist temporal writes shrink to decay/sustain/release (attack pinned at
20ms; input rise/fall deleted; slew fixed). Verify candidate parameters by
running them through the engine offline on a click train at the measured IOI
before committing. Validation corpus: 5-10 labeled real tracks incl. a
noise-floor capture that MUST produce no lock.

**Skeptic corrections that bind (do not implement the design literally):**
- All temporal formulas must use the FOLDED beat (fold_to_perceptual_beat)
  — the raw-IOI spec in the design would regress the octave fix.
- The release subtraction saturates its 80ms clamp for most dance tempos;
  raise the target floor to fixed-constants + 80*stretch first.
- run_chain (parity harness) ends at map_output — the slew and pre-fire are
  NOT golden-covered; the offline verify step needs an output-stage model.
- Dither: Weber is relative — at levels 3-5/20 one step is 20-33%, above
  JND. Do NOT disable static dither at low steady levels.
- gate_smoothing is carried by every preset; freezing it silently discards
  preset-differentiated behavior — keep it user-facing or retune values.
- micro-pause counters are per-CALL, repaint-rate-dependent — the time-based
  respec is required for correctness, not just feel.

## Phased plan (skeptic-approved ordering)

- **Phase 0** (desktop-only, no engine change, no feel change): composite
  Response readout; delete Input Smoothing UI group (freeze constants, keep
  feeding the meter AND motor2 silence check); strip input_rise/fall from
  Auto-Lock LockParams; tempo-confidence decay + pre-fire recency guard in
  BOTH BeatDetectors (verified not golden-visible — confirm with a no-diff
  golden run); comment hygiene (stale "35ms retrigger cooldown").
  Also close the pre-existing preset parity debt first.
- **Phase 1** (dual port + golden regen #1): squelch before normalization
  (needs the parity harness to gain the normalization stage, else the new
  golden scenario is theater); new noise-floor golden scenario.
- **Phase 2** (THE feel change; gated on hardware A/B): slew 85→15
  symmetric on both platforms in one commit; pre-fire 76→~50 derived from
  the constants table; Auto-Lock composite solver; factory preset VALUE
  retune (release +~56ms where the old slew tail was load-bearing); stored
  slew==85 auto-migrates, custom values clamp to 0-60; "Classic Pump"
  expert flag (NOT a preset — the Preset struct lacks the field).
- **Phase 3** (dual port): silence-class bypass flag through map_output
  metadata to the device tasks (peak-hold + slew-fall bypass); micro-pause
  time-based depth respec; climax rate caps.
- **Phase 4**: labeled-corpus integration test; measure the actual Domi 2
  spin-up/down with a phone accelerometer to replace the literature numbers.

## Riskiest assumptions (carry these into every phase)

1. Domi 2 motor physics are assumed from generic ERM literature — measure
   before Phase 2, not after.
2. The 85ms asymmetric "pump" might be the product's signature, not a
   defect — Phase 2 changes the feel of every session and is gated on one
   hardware A/B. Ship the escape hatch.
3. The perceptual constants (Weber, 54ms asynchrony, RSS composition) are
   from general literature applied to this hardware through BLE; the
   readout's numbers are estimates.
4. A better-composed rule-based Auto-Lock is still rule-based (Sound2Hap:
   rule-based mappings don't generalize) — the labeled corpus is the guard.
