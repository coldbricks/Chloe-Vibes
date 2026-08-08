# WAVE-001 — feel-depth

**Status:** VERIFY  
**Created:** 2026-07-19  
**Repo:** C:\Users\coldb\Chloe-Vibes  
**MAX COMPUTE:** ON  

## Scope (one sentence)

Improve haptic *feel* only: tempo pre-fire honesty, true-zero micro-pauses that reach motors, snappier punch, tighter lead timing — dual platform, parity-locked.

## Non-goals

- Android lifecycle / dead-man watchdog safety (separate wave)
- FIND BOOM Android port
- CI / release packaging
- UI declutter beyond what's required for feel knobs
- Full Phase-2 slew redesign (Classic Pump inversion) without hardware A/B

## Ownership

| Owner | Paths | Role |
|-------|-------|------|
| Grok | `src/audio.rs`, `src/gui.rs`, `src/settings.rs`, `tests/*`, Android audio + BLE output, WAVE | builder + integrator |

## Frozen interfaces

- Signal chain order: Spectral → Gate → Beat → Envelope → Climax → Output
- Lovense 0–20 protocol
- Parity golden columns / scenarios list

## Acceptance commands

```text
cargo test
cargo test --test parity
cd android && ./gradlew testDebugUnitTest
```

## Dispatch plan

- Single builder (engine ports must stay lockstep)
- Self-verify with suite; no product scope creep

## Ledger (live)

### Evidence

- `cargo test` green (incl. new decay + micro-pause tests)
- `cargo test --test parity` green (golden regenerated)
- `cargo clippy -- -D warnings` green
- `gradlew testDebugUnitTest` green (Kotlin parity)

### Implemented

1. Tempo confidence decay + prefire recency; lead 76→50 ms
2. Time-based micro-pauses (60–100 ms) + `silence_event`
3. Silence hard-snap past slew (desktop + Android); Android peak-hold bypass on 0
4. Punchier velocity overshoot; large-jump fast rise (0.15× slew)
5. Default output slew 48→42 ms (deeper boom trough)
6. Time-based sustain magnitude smoother

### Open risks

- Pre-fire 50 ms may feel early on slow BLE; constant is `BeatDetector::PREFIRE_LEAD_MS`
- Full Phase-2 slew redesign (symmetric 15 ms / Classic Pump) deferred — needs Domi A/B

### Close

- Observer: Grok (suite) — hardware A/B still David  
- Commit: (pending user)  
- Result: VERIFY → close after on-device feel check  
