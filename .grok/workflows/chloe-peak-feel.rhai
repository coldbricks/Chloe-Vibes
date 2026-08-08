// ChloeVibes Peak Feel Army — full-app audit → ranked plan → implement → verify
// Goal: maximize haptic intensity, musical contrast, climax arc, and motor honesty
// so the product feels incredible on real Lovense hardware (Domi primary).
// Repo: C:\Users\coldb\Chloe-Vibes  |  Branch: master  |  Version baseline: 1.5.0
// Invariants: signal chain order fixed; Lovense 0-20; dual-client parity; no commit/push.

let meta = #{
    name: "chloe-peak-feel",
    description: "Full-app army: audit feel/climax/BLE/safety/UX/presets, implement peak haptic improvements, verify parity",
    when_to_use: "Make ChloeVibes feel as incredible as possible; peak climax/haptic experience pass",
    phases: [
        #{ title: "Map", detail: "inventory + WIP + version ground truth" },
        #{ title: "Audit", detail: "parallel specialists across the whole app" },
        #{ title: "Plan", detail: "rank and write WAVE-002 peak-feel plan" },
        #{ title: "Implement", detail: "high-impact feel + safety + climax improvements" },
        #{ title: "Verify", detail: "cargo + gradle tests + adversarial feel review" },
    ],
};

let inventory_schema = #{
    "type": "object",
    "required": ["summary", "version_ok", "wip_summary", "hot_files"],
    "properties": #{
        "summary": #{ "type": "string" },
        "version_ok": #{ "type": "boolean" },
        "wip_summary": #{ "type": "string" },
        "hot_files": #{ "type": "array", "items": #{ "type": "string" }, "maxItems": 20 },
    },
};

let audit_schema = #{
    "type": "object",
    "required": ["domain", "findings", "top_actions"],
    "properties": #{
        "domain": #{ "type": "string" },
        "findings": #{
            "type": "array",
            "maxItems": 10,
            "items": #{
                "type": "object",
                "required": ["severity", "file", "issue", "why_it_matters_for_feel"],
                "properties": #{
                    "severity": #{ "type": "string" },
                    "file": #{ "type": "string" },
                    "issue": #{ "type": "string" },
                    "why_it_matters_for_feel": #{ "type": "string" },
                },
            },
        },
        "top_actions": #{
            "type": "array",
            "maxItems": 6,
            "items": #{
                "type": "object",
                "required": ["title", "impact", "effort", "files"],
                "properties": #{
                    "title": #{ "type": "string" },
                    "impact": #{ "type": "string" },
                    "effort": #{ "type": "string" },
                    "files": #{ "type": "string" },
                },
            },
        },
    },
};

let plan_schema = #{
    "type": "object",
    "required": ["wave_path", "must_ship", "nice", "deferred", "summary"],
    "properties": #{
        "wave_path": #{ "type": "string" },
        "must_ship": #{ "type": "array", "items": #{ "type": "string" }, "maxItems": 12 },
        "nice": #{ "type": "array", "items": #{ "type": "string" }, "maxItems": 12 },
        "deferred": #{ "type": "array", "items": #{ "type": "string" }, "maxItems": 12 },
        "summary": #{ "type": "string" },
    },
};

let implement_schema = #{
    "type": "object",
    "required": ["lane", "changed_files", "behavior_delta", "tests_run", "risks"],
    "properties": #{
        "lane": #{ "type": "string" },
        "changed_files": #{ "type": "array", "items": #{ "type": "string" }, "maxItems": 20 },
        "behavior_delta": #{ "type": "string" },
        "tests_run": #{ "type": "string" },
        "risks": #{ "type": "string" },
    },
};

let verify_schema = #{
    "type": "object",
    "required": ["cargo_ok", "parity_ok", "android_tests_ok", "notes", "ship_ready"],
    "properties": #{
        "cargo_ok": #{ "type": "boolean" },
        "parity_ok": #{ "type": "boolean" },
        "android_tests_ok": #{ "type": "boolean" },
        "notes": #{ "type": "string" },
        "ship_ready": #{ "type": "boolean" },
    },
};

let root = "C:\\Users\\coldb\\Chloe-Vibes";

// ─── Phase: Map ─────────────────────────────────────────────────────────────
phase("Map");
log("Mapping ChloeVibes ground truth + WIP");

let map_prompt = "";
map_prompt += "You are the ground-truth mapper for ChloeVibes at " + root + ".\n";
map_prompt += "Use read_file, list_dir, grep, and shell (git status, git log, VERSION) only.\n";
map_prompt += "Confirm: local VERSION/Cargo/Android vs GitHub origin/master and release v1.5.0.\n";
map_prompt += "Summarize uncommitted WIP (WAVE-001 feel-depth, FinallyHapticBridge, etc).\n";
map_prompt += "List hot files for feel (audio.rs, EnvelopeProcessor, ClimaxEngine, BeatDetector, BleDeviceManager, gui.rs, presets, MainScreen).\n";
map_prompt += "Do NOT edit files. Return structured inventory only.\n";

let map_r = agent(map_prompt, #{
    label: "map-ground-truth",
    capability_mode: "read-only",
    output_schema: inventory_schema,
});

if map_r == () || !map_r.success {
    log("Map agent failed; continuing with known root");
} else {
    log("Map: " + map_r.output.summary);
}

// ─── Phase: Audit (parallel specialists) ────────────────────────────────────
phase("Audit");
log("Dispatching full-app audit army");

let domains = [
    "HAPTIC_FEEL_ENVELOPE",
    "CLIMAX_EDGE_DENY",
    "BEAT_PREFIRE_TEMPO",
    "BLE_LATENCY_DEVICE_CURVES",
    "ANDROID_SAFETY_WATCHDOG",
    "PRESETS_DEFAULT_PATH",
    "DESKTOP_UI_FIND_BOOM",
    "ANDROID_UI_UX",
    "PARITY_GOLDEN_WIP",
    "ANTI_ADAPTATION_MICROPAUSES",
    "OUTPUT_SLEW_TEMPORAL",
    "SESSION_ARC_AROUSAL",
];

let audit_jobs = [];
for d in domains {
    let p = "";
    p += "You are a specialist auditor for ChloeVibes domain: " + d + ".\n";
    p += "Repo root: " + root + "\n";
    p += "Product goal: the app must feel the most incredible possible on real Lovense hardware —\n";
    p += "maximum musical punch, deep boom contrast, honest motor rest, devastating climax arcs,\n";
    p += "anti-adaptation that keeps sensation alive, low latency, safe stop. Adult haptic software.\n";
    p += "Rules:\n";
    p += "- READ the actual code with read_file/grep before any claim. No memory-only answers.\n";
    p += "- Signal chain order is frozen: Spectral→Gate→Beat→Envelope→Climax→Output.\n";
    p += "- Dual clients must stay parity-locked (Rust src/audio.rs ↔ Kotlin audio/*).\n";
    p += "- Respect docs/TEMPORAL_ARCHITECTURE.md, docs/waves/WAVE-001.md, docs/TECHNICAL_REFERENCE.md.\n";
    p += "- Local WIP may already include WAVE-001 feel fixes — audit working tree, not only HEAD.\n";
    p += "- Severity: critical|high|medium|low. Prefer concrete file:line-level issues.\n";
    p += "- top_actions must be implementable this session if high impact.\n";
    p += "- Do NOT edit files. Return domain, findings[], top_actions[].\n";
    p += "Domain focus hints:\n";
    if d == "HAPTIC_FEEL_ENVELOPE" {
        p += "ADSR, velocity overshoot, silence_event, boom/pluck rest, EnvelopeProcessor both platforms.\n";
    }
    if d == "CLIMAX_EDGE_DENY" {
        p += "ClimaxEngine cycles, edge-and-deny, arousal momentum, surge/tease, experience presets.\n";
    }
    if d == "BEAT_PREFIRE_TEMPO" {
        p += "BeatDetector PREFIRE_LEAD_MS, confidence decay, recency guard, ghost-fire risks.\n";
    }
    if d == "BLE_LATENCY_DEVICE_CURVES" {
        p += "BleDeviceManager throttle, rest floors, power curves, Domi hang, dual-motor Edge.\n";
    }
    if d == "ANDROID_SAFETY_WATCHDOG" {
        p += "Missing Android dead-man watchdog vs desktop; stop-all; reconnect; lifecycle.\n";
    }
    if d == "PRESETS_DEFAULT_PATH" {
        p += "Bass Drum default, BOOM macros, climax-off path, Presets.kt + presets.rs parity.\n";
    }
    if d == "DESKTOP_UI_FIND_BOOM" {
        p += "gui.rs FIND BOOM, OVERRIDE collapse, meters honesty, dead sliders.\n";
    }
    if d == "ANDROID_UI_UX" {
        p += "MainScreen Compose UX, one-hand control, climax controls, clarity under arousal.\n";
    }
    if d == "PARITY_GOLDEN_WIP" {
        p += "tests/parity.rs, parity_golden.csv, Kotlin unit tests, drift risks in WIP.\n";
    }
    if d == "ANTI_ADAPTATION_MICROPAUSES" {
        p += "Micro-pauses reach motor? peak-hold bypass? sustain modulation rates vs motor bandwidth.\n";
    }
    if d == "OUTPUT_SLEW_TEMPORAL" {
        p += "Phase-2 slew redesign readiness; 0.35 rise ratio; Classic Pump; TEMPORAL_ARCHITECTURE Phase 2.\n";
    }
    if d == "SESSION_ARC_AROUSAL" {
        p += "Long-session feel, climax maturity, edge-deny timing, boredom/adaptation failure modes.\n";
    }
    audit_jobs.push(#{
        prompt: p,
        label: "audit:" + d,
        capability_mode: "read-only",
        output_schema: audit_schema,
    });
}

let audits = parallel(audit_jobs);

let audit_blob = "";
let audit_ok = 0;
let i = 0;
for r in audits {
    if r != () && r.success && r.output.domain != () {
        audit_ok += 1;
        audit_blob += "### " + r.output.domain + "\n";
        if r.output.findings != () {
            for f in r.output.findings {
                audit_blob += "- [" + f.severity + "] " + f.file + ": " + f.issue + "\n";
                audit_blob += "  feel: " + f.why_it_matters_for_feel + "\n";
            }
        }
        if r.output.top_actions != () {
            for a in r.output.top_actions {
                audit_blob += "  ACTION (" + a.impact + "/" + a.effort + "): " + a.title + " → " + a.files + "\n";
            }
        }
        audit_blob += "\n";
    }
    i += 1;
}
log("Audits succeeded: " + audit_ok.to_string() + "/" + domains.len().to_string());

let scratch_audits = write_scratch_file("audit_bundle.md", audit_blob);

// ─── Phase: Plan ────────────────────────────────────────────────────────────
phase("Plan");
log("Synthesizing WAVE-002 peak-feel plan");

let plan_prompt = "";
plan_prompt += "You are the peak-feel planner for ChloeVibes at " + root + ".\n";
plan_prompt += "Read docs/waves/WAVE-001.md, docs/TEMPORAL_ARCHITECTURE.md, CLAUDE.md, and the audit bundle below.\n";
plan_prompt += "Write docs/waves/WAVE-002-PEAK-FEEL.md with: goal, non-goals, must-ship actions (ordered),\n";
plan_prompt += "implementation notes per action (files, parity requirements), acceptance commands, risks.\n";
plan_prompt += "Product goal: hardest, most incredible haptic climax experience — punch, contrast, arc, honesty.\n";
plan_prompt += "Prioritize: (1) motor-honest silence/contrast (2) climax edge-deny excellence (3) latency/prefire\n";
plan_prompt += "(4) Android safety watchdog (5) preset/session arc (6) UX under arousal.\n";
plan_prompt += "Do NOT implement code yet. Create the wave doc. Return wave_path, must_ship, nice, deferred, summary.\n";
plan_prompt += "\n--- AUDIT BUNDLE ---\n";
plan_prompt += audit_blob;

let plan_r = agent(plan_prompt, #{
    label: "plan-wave-002",
    capability_mode: "read-write",
    output_schema: plan_schema,
});

let plan_summary = "plan unavailable";
if plan_r != () && plan_r.success && plan_r.output.summary != () {
    plan_summary = plan_r.output.summary;
    log("Plan: " + plan_summary);
}

// ─── Phase: Implement (parallel lanes; avoid same-file conflicts) ───────────
phase("Implement");
log("Implementing high-impact peak-feel lanes");

let impl_jobs = [];

// Lane A: Engine feel parity (Rust + Kotlin audio) — single agent owns both
let pa = "";
pa += "IMPLEMENTATION LANE A — ENGINE FEEL (parity-locked) for ChloeVibes at " + root + ".\n";
pa += "Read WAVE-002 if present (docs/waves/WAVE-002-PEAK-FEEL.md), WAVE-001, TEMPORAL_ARCHITECTURE, CLAUDE.md.\n";
pa += "Implement the highest-impact engine feel improvements that do NOT require hardware A/B for Phase-2 full slew redesign.\n";
pa += "Focus candidates (verify against code first; skip if already done in WIP):\n";
pa += "1) Ensure silence_event / micro-pauses hard-snap past slew on BOTH platforms and reach motors.\n";
pa += "2) ClimaxEngine: stronger edge-and-deny contrast, better surge/tease maturity curve, motor-bandwidth-safe rates (<=5Hz meaningful).\n";
pa += "3) BeatDetector: confidence decay + recency already in WIP — tighten if still ghost-fire risk; keep PREFIRE ~50ms unless evidence says otherwise.\n";
pa += "4) Velocity overshoot / boom trough contrast improvements if not already at best.\n";
pa += "5) Update parity golden if map_output-affecting behavior changes; keep tests green.\n";
pa += "Rules: chain order frozen; dual-port lockstep; no commit/push; match existing style; run cargo test and parity if possible.\n";
pa += "Edit only engine files + tests as needed. Report changed_files, behavior_delta, tests_run, risks.\n";
impl_jobs.push(#{
    prompt: pa,
    label: "impl:engine-feel",
    capability_mode: "all",
    output_schema: implement_schema,
});

// Lane B: Android safety watchdog
let pb = "";
pb += "IMPLEMENTATION LANE B — ANDROID SAFETY WATCHDOG for ChloeVibes at " + root + ".\n";
pb += "Desktop has dead-man watchdog (2s pipeline stall → stop devices). Android does not (TECHNICAL_REFERENCE).\n";
pb += "Implement a robust Android equivalent: if audio pipeline / output path stalls, force motors to 0 and surface UI state.\n";
pb += "Read MainActivity, AudioCaptureManager, BleDeviceManager, ChloeVibesApplication, FinallyHapticBridgeService.\n";
pb += "Do not break Finally bridge if present. No commit/push. Match project style. Prefer minimal correct change.\n";
pb += "Report changed_files, behavior_delta, tests_run, risks.\n";
impl_jobs.push(#{
    prompt: pb,
    label: "impl:android-watchdog",
    capability_mode: "all",
    output_schema: implement_schema,
});

// Lane C: Device curves + BLE honesty
let pc = "";
pc += "IMPLEMENTATION LANE C — DEVICE CURVES + BLE HONESTY for ChloeVibes at " + root + ".\n";
pc += "Improve Domi/wand rest floors, power curves, and BLE command throttling so motors stop cleanly and punch hard.\n";
pc += "Read BleDeviceManager.kt, LovenseProtocol.kt, gui.rs device mapping (MotorKind rest_floor/power).\n";
pc += "Avoid reintroducing Domi hang (residual gate re-arm). Prefer true-zero rest. No commit/push.\n";
pc += "Report changed_files, behavior_delta, tests_run, risks.\n";
impl_jobs.push(#{
    prompt: pc,
    label: "impl:device-ble",
    capability_mode: "all",
    output_schema: implement_schema,
});

// Lane D: Presets + climax session arc
let pd = "";
pd += "IMPLEMENTATION LANE D — PRESETS + SESSION ARC for ChloeVibes at " + root + ".\n";
pd += "Make default and climax-oriented presets devastatingly good: Bass Drum boom path stays king for music;\n";
pd += "add/refine an experience preset path that builds arousal and edges hard without neural adaptation death.\n";
pd += "Parity: Presets.kt and src/presets.rs must stay aligned on shared parameters.\n";
pd += "No commit/push. Report changed_files, behavior_delta, tests_run, risks.\n";
impl_jobs.push(#{
    prompt: pd,
    label: "impl:presets-arc",
    capability_mode: "all",
    output_schema: implement_schema,
});

// Lane E: Android UX under arousal
let pe = "";
pe += "IMPLEMENTATION LANE E — ANDROID UX UNDER AROUSAL for ChloeVibes at " + root + ".\n";
pe += "MainScreen.kt: make Stop all, intensity ceiling, FIND/preset path, and climax controls idiot-proof under high arousal.\n";
pe += "Bigger hit targets, clearer active state, less clutter, no accidental climax disable mid-edge if avoidable.\n";
pe += "Do not redesign entire app chrome. No commit/push. Report changed_files, behavior_delta, tests_run, risks.\n";
impl_jobs.push(#{
    prompt: pe,
    label: "impl:android-ux",
    capability_mode: "all",
    output_schema: implement_schema,
});

let impls = parallel(impl_jobs);

let impl_report = "";
let impl_ok = 0;
for r in impls {
    if r != () && r.success && r.output.lane != () {
        impl_ok += 1;
        impl_report += "## " + r.output.lane + "\n";
        impl_report += "delta: " + r.output.behavior_delta + "\n";
        impl_report += "tests: " + r.output.tests_run + "\n";
        impl_report += "risks: " + r.output.risks + "\n";
        if r.output.changed_files != () {
            for f in r.output.changed_files {
                impl_report += "- " + f + "\n";
            }
        }
        impl_report += "\n";
    }
}
log("Implement lanes ok: " + impl_ok.to_string() + "/5");
let scratch_impl = write_scratch_file("implement_report.md", impl_report);

// ─── Phase: Verify ──────────────────────────────────────────────────────────
phase("Verify");
log("Running verification army");

let vjobs = [];

let pv = "";
pv += "VERIFIER: run tests for ChloeVibes at " + root + ".\n";
pv += "Commands:\n";
pv += "  cd " + root + " ; cargo test\n";
pv += "  cd " + root + " ; cargo test --test parity\n";
pv += "  cd " + root + "\\android ; .\\gradlew testDebugUnitTest\n";
pv += "Optionally cargo clippy -- -D warnings if time allows.\n";
pv += "Fix only trivial breakages you introduced if tests fail and you can safely fix; else report failures.\n";
pv += "Return cargo_ok, parity_ok, android_tests_ok, notes, ship_ready.\n";
vjobs.push(#{
    prompt: pv,
    label: "verify:tests",
    capability_mode: "all",
    output_schema: verify_schema,
});

let padv = "";
padv += "ADVERSARIAL FEEL REVIEWER for ChloeVibes at " + root + ".\n";
padv += "Read git diff of working tree. Challenge every claim that feel improved.\n";
padv += "Look for: parity breaks, Domi hang regressions, dead UI, ghost pre-fire, climax that adapts to mush,\n";
padv += "Android watchdog false positives, BLE spam, safety regressions.\n";
padv += "Write docs/waves/WAVE-002-VERIFY.md with pass/fail per lane and residual risks.\n";
padv += "Return cargo_ok=false if you did not run tests; set ship_ready only if code review is clean AND you believe tests should pass.\n";
padv += "Fields: cargo_ok, parity_ok, android_tests_ok (use false if unknown), notes (full review), ship_ready.\n";
vjobs.push(#{
    prompt: padv,
    label: "verify:adversarial-feel",
    capability_mode: "all",
    output_schema: verify_schema,
});

let vers = parallel(vjobs);

let final_notes = "Peak-feel army complete.\nPlan: " + plan_summary + "\n";
final_notes += "Implement lanes: " + impl_ok.to_string() + "/5\n";
final_notes += "Audit domains: " + audit_ok.to_string() + "/" + domains.len().to_string() + "\n";

let ship = false;
for r in vers {
    if r != () && r.success {
        final_notes += r.output.notes + "\n";
        if r.output.ship_ready == true {
            ship = true;
        }
    }
}

let report = "";
report += "# ChloeVibes Peak Feel Army — Run Report\n\n";
report += final_notes + "\n";
report += "## Implement report\n" + impl_report + "\n";
report += "## Audit (abbrev)\n" + audit_blob + "\n";
let report_path = write_scratch_file("peak_feel_report.md", report);

// Also try to persist a copy into the repo docs for the user
let persist = "";
persist += "Write the final army report to " + root + "\\docs\\waves\\WAVE-002-ARMY-REPORT.md\n";
persist += "Content to include (expand cleanly):\n" + report + "\n";
persist += "Then return a one-line confirmation as notes; set ship_ready=" + ship.to_string() + ".\n";
persist += "cargo_ok/parity_ok/android_tests_ok: copy best knowledge from context or false.\n";
let _persist_r = agent(persist, #{
    label: "persist-report",
    capability_mode: "read-write",
    output_schema: verify_schema,
});

complete(#{
    summary: final_notes,
    report_scratch: report_path,
    audits_ok: audit_ok,
    impl_ok: impl_ok,
    ship_ready: ship,
});
