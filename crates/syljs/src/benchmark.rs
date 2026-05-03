#![allow(clippy::too_many_lines)]
#![doc = "Synthetic YouTube-like benchmark harness and research metric aggregation."]

use crate::{
    compile_program, dom::DomHost, install_canvas_globals, install_cssom_globals,
    install_dom_globals, install_media_globals, install_web_api_globals, install_worker_globals,
    media::MediaHost, parse_script, CanvasCommand, CanvasHost, CanvasMetrics, CssStyleMutation,
    CssomHost, CssomMetrics, DomBindingMetrics, EventLoopConfig, EventLoopRunSummary,
    JsRuntimeError, MediaEventRecord, MediaMetrics, MediaSegmentRecord, ProgramKind,
    ResearchCanvasHost, ResearchCssomHost, ResearchDom, ResearchMediaHost, ResearchWebApiHost,
    ResearchWorkerHost, ScheduledVm, VmConfig, WebApiHost, WebApiMetrics, WebApiResponse,
    WorkerHost, WorkerMetrics,
};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::time::Instant;

/// Benchmark profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BenchmarkProfile {
    /// Small smoke test profile.
    Smoke,

    /// Default paper-friendly profile.
    Standard,

    /// Heavier stress profile.
    Stress,
}

impl Default for BenchmarkProfile {
    fn default() -> Self {
        Self::Standard
    }
}

/// Named benchmark scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkScenario {
    /// Scenario id.
    pub id: String,

    /// Human-readable title.
    pub title: String,

    /// Scenario description.
    pub description: String,
}

impl Default for BenchmarkScenario {
    fn default() -> Self {
        Self {
            id: "synthetic-tube-watch".to_owned(),
            title: "SyntheticTube watch page".to_owned(),
            description: "YouTube-like page boot, player setup, manifest fetch, MSE buffering, canvas overlay rendering, storage/history, and Worker-lite decode messages.".to_owned(),
        }
    }
}

/// Benchmark configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Profile.
    pub profile: BenchmarkProfile,

    /// Frame count drawn through Canvas 2D.
    pub frame_count: u32,

    /// Synthetic media segments fetched/appended.
    pub segment_count: u32,

    /// Worker messages posted.
    pub worker_message_count: u32,

    /// DOM recommendation card count.
    pub recommendation_count: u32,

    /// Style mutation iterations.
    pub style_mutation_count: u32,

    /// Timer interval in ms.
    pub timer_interval_ms: u64,

    /// VM instruction budget.
    pub instruction_budget: u64,

    /// VM call-depth limit.
    pub max_call_depth: usize,

    /// Event loop job budget.
    pub max_jobs_per_run: u64,

    /// Whether timers auto-advance.
    pub auto_advance_time: bool,

    /// Benchmark scenario metadata.
    pub scenario: BenchmarkScenario,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self::for_profile(BenchmarkProfile::Standard)
    }
}

impl BenchmarkConfig {
    /// Creates a config for a named profile.
    #[must_use]
    pub fn for_profile(profile: BenchmarkProfile) -> Self {
        match profile {
            BenchmarkProfile::Smoke => Self {
                profile,
                frame_count: 4,
                segment_count: 2,
                worker_message_count: 2,
                recommendation_count: 4,
                style_mutation_count: 3,
                timer_interval_ms: 8,
                instruction_budget: 150_000,
                max_call_depth: 128,
                max_jobs_per_run: 2_000,
                auto_advance_time: true,
                scenario: BenchmarkScenario::default(),
            },
            BenchmarkProfile::Standard => Self {
                profile,
                frame_count: 12,
                segment_count: 4,
                worker_message_count: 4,
                recommendation_count: 12,
                style_mutation_count: 8,
                timer_interval_ms: 16,
                instruction_budget: 750_000,
                max_call_depth: 256,
                max_jobs_per_run: 10_000,
                auto_advance_time: true,
                scenario: BenchmarkScenario::default(),
            },
            BenchmarkProfile::Stress => Self {
                profile,
                frame_count: 48,
                segment_count: 12,
                worker_message_count: 16,
                recommendation_count: 32,
                style_mutation_count: 32,
                timer_interval_ms: 16,
                instruction_budget: 3_000_000,
                max_call_depth: 512,
                max_jobs_per_run: 50_000,
                auto_advance_time: true,
                scenario: BenchmarkScenario::default(),
            },
        }
    }

    /// Returns VM config.
    #[must_use]
    pub fn vm_config(&self) -> VmConfig {
        VmConfig {
            instruction_budget: self.instruction_budget,
            max_call_depth: self.max_call_depth,
        }
    }

    /// Returns event-loop config.
    #[must_use]
    pub fn event_loop_config(&self) -> EventLoopConfig {
        EventLoopConfig {
            max_jobs_per_run: self.max_jobs_per_run,
            max_timer_advances: self.max_jobs_per_run,
            auto_advance_time: self.auto_advance_time,
        }
    }

    /// Sanitizes values that could otherwise create nonsense workloads.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.frame_count = self.frame_count.clamp(1, 10_000);
        self.segment_count = self.segment_count.clamp(1, 1_000);
        self.worker_message_count = self.worker_message_count.clamp(0, 10_000);
        self.recommendation_count = self.recommendation_count.clamp(0, 5_000);
        self.style_mutation_count = self.style_mutation_count.clamp(0, 10_000);
        self.timer_interval_ms = self.timer_interval_ms.min(1_000);
        self.instruction_budget = self.instruction_budget.max(1_000);
        self.max_jobs_per_run = self.max_jobs_per_run.max(128);
        self
    }
}

/// Console line with index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkConsoleRecord {
    /// Console index.
    pub index: usize,

    /// Console line.
    pub line: String,
}

/// Script summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkScriptSummary {
    /// Script kind.
    pub kind: ProgramKind,

    /// Script byte length.
    pub bytes: usize,

    /// Estimated line count.
    pub lines: usize,

    /// Expected frame count.
    pub expected_frames: u32,

    /// Expected media segment count.
    pub expected_segments: u32,

    /// Expected worker messages.
    pub expected_worker_messages: u32,
}

/// Aggregate benchmark metrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkAggregateMetrics {
    /// Wall-clock benchmark runtime in microseconds.
    pub wall_time_us: u128,

    /// VM instructions.
    pub vm_instructions: u64,

    /// VM function calls.
    pub vm_calls: u64,

    /// VM native calls.
    pub vm_native_calls: u64,

    /// VM bytecode calls.
    pub vm_bytecode_calls: u64,

    /// Event loop jobs executed.
    pub event_loop_jobs: u64,

    /// Microtasks executed.
    pub microtasks_executed: u64,

    /// Promise reactions executed.
    pub promise_reactions_executed: u64,

    /// Timers fired.
    pub timers_fired: u64,

    /// DOM mutations.
    pub dom_mutations: u64,

    /// DOM queries.
    pub dom_queries: u64,

    /// CSSOM mutations.
    pub cssom_mutations: u64,

    /// CSSOM computed style reads.
    pub computed_style_reads: u64,

    /// Fetch calls.
    pub fetch_calls: u64,

    /// Storage writes.
    pub storage_writes: u64,

    /// History mutations.
    pub history_mutations: u64,

    /// Media buffer appends.
    pub media_buffer_appends: u64,

    /// Media events.
    pub media_events: u64,

    /// Canvas commands recorded.
    pub canvas_commands_recorded: u64,

    /// Worker messages total.
    pub worker_messages_total: u64,

    /// Console line count.
    pub console_lines: usize,

    /// Whether event loop hit a configured limit.
    pub hit_limit: bool,
}

/// Benchmark run result.
#[derive(Debug, Clone)]
pub struct BenchmarkRun {
    /// Config used.
    pub config: BenchmarkConfig,

    /// Generated script summary.
    pub script: BenchmarkScriptSummary,

    /// Event-loop summary.
    pub summary: EventLoopRunSummary,

    /// DOM metrics.
    pub dom: DomBindingMetrics,

    /// CSSOM metrics.
    pub cssom: CssomMetrics,

    /// Web API metrics.
    pub web_api: WebApiMetrics,

    /// Media metrics.
    pub media: MediaMetrics,

    /// Canvas metrics.
    pub canvas: CanvasMetrics,

    /// Worker metrics.
    pub workers: WorkerMetrics,

    /// Aggregate metrics.
    pub aggregate: BenchmarkAggregateMetrics,

    /// Captured console records.
    pub console: Vec<BenchmarkConsoleRecord>,

    /// Canvas command records.
    pub canvas_commands: Vec<CanvasCommand>,

    /// CSS style mutations.
    pub style_mutations: Vec<CssStyleMutation>,

    /// Media event records.
    pub media_events: Vec<MediaEventRecord>,

    /// Media segment records.
    pub media_segments: Vec<MediaSegmentRecord>,
}

/// Builds the synthetic YouTube-like workload script.
#[must_use]
pub fn build_youtube_like_script(config: &BenchmarkConfig) -> String {
    let config = config.clone().sanitized();

    format!(
        r##"
document.title = "SyntheticTube Watch";
history.pushState({{}}, "", "/watch?v=sylphos-benchmark");
localStorage.setItem("sylphos.autoplay", "1");
sessionStorage.setItem("sylphos.session", "benchmark");

document.styleSheets[0].insertRule(".synthetic-shell {{ color: #f1f1f1; background-color: #0f0f0f; }}", 0);
document.styleSheets[0].insertRule(".recommendation {{ color: #cccccc; font-size: 14px; }}", 1);
document.styleSheets[0].insertRule(".hot {{ color: #ffffff; }}", 2);

const shell = document.createElement("div");
shell.id = "synthetic-root";
shell.className = "synthetic-shell";
shell.style.width = "1280px";
shell.style.padding = "16px";
document.body.appendChild(shell);

const title = document.createElement("h1");
title.id = "watch-title";
title.textContent = "Sylphos Synthetic Playback Benchmark";
shell.appendChild(title);

const stats = document.createElement("p");
stats.id = "stats";
stats.textContent = "booting";
shell.appendChild(stats);

const player = new HTMLVideoElement();
player.id = "player";
player.className = "media-player";
player.style.width = "1280px";
player.style.height = "720px";
player.controls = true;
shell.appendChild(player);

const overlay = new HTMLCanvasElement();
overlay.id = "overlay";
overlay.width = 1280;
overlay.height = 720;
overlay.style.width = "1280px";
overlay.style.height = "720px";
shell.appendChild(overlay);

const ctx = overlay.getContext("2d");
ctx.font = "20px sans-serif";
ctx.fillStyle = "#ffffff";
ctx.strokeStyle = "#ff0000";
ctx.lineWidth = 2;

const mediaSource = new MediaSource();
const objectUrl = URL.createObjectURL(mediaSource);
player.src = objectUrl;
const sourceBuffer = mediaSource.addSourceBuffer("video/mp4; codecs=\"avc1.42E01E\"");

const worker = new Worker("synthetic-decoder-worker.js");
worker.onmessage = function (event) {{
    console.log("worker-message", event.data);
}};

player.addEventListener("play", function (event) {{
    console.log("media-event", event.type);
}});

player.addEventListener("timeupdate", function (event) {{
    console.log("timeupdate", player.currentTime);
}});

fetch("/api/player/manifest")
    .then(function (response) {{
        response.text().then(function (manifest) {{
            localStorage.setItem("manifest", manifest);
            stats.textContent = "manifest-loaded";
            console.log("manifest", manifest);
        }});
    }});

let recommendationIndex = 0;
while (recommendationIndex < {recommendation_count}) {{
    const card = document.createElement("div");
    card.className = "recommendation";
    card.textContent = "Recommended video " + recommendationIndex;
    shell.appendChild(card);
    recommendationIndex = recommendationIndex + 1;
}}

let styleIndex = 0;
while (styleIndex < {style_mutation_count}) {{
    title.style.fontSize = (20 + styleIndex) + "px";
    title.style.color = "#ffffff";
    const computedColor = getComputedStyle(title).color;
    if (styleIndex == 0) {{
        console.log("computed-color", computedColor);
    }}
    styleIndex = styleIndex + 1;
}}

let segmentIndex = 0;
function fetchNextSegment() {{
    if (segmentIndex >= {segment_count}) {{
        mediaSource.endOfStream();
        stats.textContent = "buffered";
        console.log("buffer-complete", player.buffered.length);
        return;
    }}

    const nextUrl = "/media/seg-" + segmentIndex;
    fetch(nextUrl).then(function (response) {{
        response.text().then(function (segment) {{
            sourceBuffer.appendBuffer(segment);
            worker.postMessage("decode-" + segmentIndex);
            console.log("segment", segmentIndex, player.buffered.length);
            segmentIndex = segmentIndex + 1;
            queueMicrotask(fetchNextSegment);
        }});
    }});
}}

fetchNextSegment();

let workerIndex = 0;
while (workerIndex < {worker_message_count}) {{
    worker.postMessage("analytics-" + workerIndex);
    workerIndex = workerIndex + 1;
}}

function drawFrame(index) {{
    ctx.clearRect(0, 0, 1280, 720);
    ctx.fillStyle = "#111111";
    ctx.fillRect(0, 0, 1280, 720);
    ctx.fillStyle = "#ffffff";
    ctx.fillText("SyntheticTube frame " + index, 40, 60);
    ctx.strokeStyle = "#ff0033";
    ctx.strokeRect(32 + index, 80, 320, 180);
    ctx.beginPath();
    ctx.moveTo(40, 320);
    ctx.lineTo(400, 320 + index);
    ctx.stroke();
}}

let frame = 0;
const frameTimer = setInterval(function () {{
    drawFrame(frame);
    player.currentTime = frame;
    frame = frame + 1;

    if (frame >= {frame_count}) {{
        clearInterval(frameTimer);
        const data = overlay.toDataURL("image/png");
        console.log("frames-complete", frame, data);
    }}
}}, {timer_interval_ms});

player.play().then(function () {{
    console.log("play-promise", player.readyState);
}});

queueMicrotask(function () {{
    console.log("microtask-checkpoint", document.title);
}});

console.log("benchmark-start", document.title);
"##,
        recommendation_count = config.recommendation_count,
        style_mutation_count = config.style_mutation_count,
        segment_count = config.segment_count,
        worker_message_count = config.worker_message_count,
        frame_count = config.frame_count,
        timer_interval_ms = config.timer_interval_ms,
    )
}

/// Runs the synthetic YouTube-like benchmark.
pub fn run_youtube_like_benchmark(config: BenchmarkConfig) -> Result<BenchmarkRun, JsRuntimeError> {
    let config = config.sanitized();
    let started = Instant::now();

    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom("Sylphos Benchmark", cssom.clone()));
    let web = Rc::new(ResearchWebApiHost::new(
        "https://synthetic.tube/watch?v=sylphos",
    ));
    let media = Rc::new(ResearchMediaHost::default());
    let canvas = Rc::new(ResearchCanvasHost::default());
    let workers = Rc::new(ResearchWorkerHost::default());

    register_benchmark_routes(&web, &config);

    let mut scheduled = ScheduledVm::with_config(config.vm_config(), config.event_loop_config());

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );
    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), web.clone());

    // The constructors are used for video/canvas in the workload so createElement
    // override ordering cannot corrupt the benchmark. Yes, this is intentional;
    // the web platform is already enough of a maze without benchmarking override roulette.
    install_media_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        media.clone(),
        Some(dom.clone()),
    );
    install_canvas_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        canvas.clone(),
        Some(dom.clone()),
    );
    install_worker_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        workers.clone(),
    );

    let script_source = build_youtube_like_script(&config);
    let script = BenchmarkScriptSummary {
        kind: ProgramKind::Script,
        bytes: script_source.len(),
        lines: script_source.lines().count(),
        expected_frames: config.frame_count,
        expected_segments: config.segment_count,
        expected_worker_messages: config.worker_message_count,
    };

    let program = parse_script(&script_source).map_err(JsRuntimeError::from_frontend_error)?;
    let bytecode = compile_program(&program, Default::default())?;
    scheduled.vm.execute(&bytecode)?;
    let summary = scheduled.run_until_idle()?;

    let dom_metrics = dom.metrics();
    let cssom_metrics = cssom.metrics();
    let web_metrics = web.metrics();
    let media_metrics = media.metrics();
    let canvas_metrics = canvas.metrics();
    let worker_metrics = workers.metrics();

    let console = summary
        .console
        .iter()
        .enumerate()
        .map(|(index, line)| BenchmarkConsoleRecord {
            index,
            line: line.clone(),
        })
        .collect::<Vec<_>>();

    let aggregate = BenchmarkAggregateMetrics {
        wall_time_us: started.elapsed().as_micros(),
        vm_instructions: summary.vm.instructions_executed,
        vm_calls: summary.vm.calls,
        vm_native_calls: summary.vm.native_calls,
        vm_bytecode_calls: summary.vm.bytecode_calls,
        event_loop_jobs: summary.jobs_executed,
        microtasks_executed: summary.event_loop.microtasks_executed,
        promise_reactions_executed: summary.event_loop.promise_reactions_executed,
        timers_fired: summary.event_loop.timers_fired,
        dom_mutations: dom_metrics
            .text_mutations
            .saturating_add(dom_metrics.attribute_mutations)
            .saturating_add(dom_metrics.value_mutations)
            .saturating_add(dom_metrics.structure_mutations),
        dom_queries: dom_metrics.queries,
        cssom_mutations: cssom_metrics
            .inline_writes
            .saturating_add(cssom_metrics.inline_removals)
            .saturating_add(cssom_metrics.rules_inserted)
            .saturating_add(cssom_metrics.rules_deleted),
        computed_style_reads: cssom_metrics.computed_reads,
        fetch_calls: web_metrics.fetch_calls,
        storage_writes: web_metrics.storage_writes,
        history_mutations: web_metrics
            .history_pushes
            .saturating_add(web_metrics.history_replaces),
        media_buffer_appends: media_metrics.buffer_appends,
        media_events: media_metrics.events,
        canvas_commands_recorded: canvas_metrics.commands_recorded,
        worker_messages_total: worker_metrics
            .messages_to_worker
            .saturating_add(worker_metrics.messages_to_main),
        console_lines: console.len(),
        hit_limit: summary.hit_limit,
    };

    Ok(BenchmarkRun {
        config,
        script,
        summary,
        dom: dom_metrics,
        cssom: cssom_metrics,
        web_api: web_metrics,
        media: media_metrics,
        canvas: canvas_metrics,
        workers: worker_metrics,
        aggregate,
        console,
        canvas_commands: canvas.commands(),
        style_mutations: cssom.mutations(),
        media_events: media.events(),
        media_segments: media.segments(),
    })
}

fn register_benchmark_routes(host: &ResearchWebApiHost, config: &BenchmarkConfig) {
    host.register_route(
        "/api/player/manifest",
        WebApiResponse::text(
            "https://synthetic.tube/api/player/manifest",
            format!(
                "manifest:segments={},frames={},workers={}",
                config.segment_count, config.frame_count, config.worker_message_count
            ),
        ),
    );

    for index in 0..config.segment_count {
        let body = synthetic_segment_payload(index);
        host.register_route(
            format!("/media/seg-{index}"),
            WebApiResponse::text(format!("https://synthetic.tube/media/seg-{index}"), body),
        );
    }
}

fn synthetic_segment_payload(index: u32) -> String {
    let mut output = String::with_capacity(1024 + index as usize * 16);
    output.push_str("sylphos-segment:");
    output.push_str(&index.to_string());
    output.push(':');

    for tick in 0..64 {
        output.push_str("frame");
        output.push_str(&tick.to_string());
        output.push(';');
    }

    output
}
