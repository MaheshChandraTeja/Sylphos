#![allow(clippy::too_many_lines)]
#![doc = "Synthetic site compatibility harness for Google/GitHub/Wikipedia-style workloads."]
#![doc = ""]
#![doc = "Module 45 adds deterministic compatibility suites that exercise the browser"]
#![doc = "surface modern sites expect: DOM construction, CSSOM mutation, fetch, storage,"]
#![doc = "history, timers, microtasks, workers, URL helpers, cookies, and app-style"]
#![doc = "boot flows. The suites are synthetic by design. Live sites are magnificent"]
#![doc = "test flakiness factories, and Sylphos deserves better than that circus."]

use crate::{
    compile_program, dom::DomHost, install_canvas_globals, install_cssom_globals,
    install_dom_globals, install_media_globals, install_web_api_globals, install_worker_globals,
    media::MediaHost, parse_script, CanvasHost, CanvasMetrics, CssomHost, CssomMetrics,
    DomBindingMetrics, EventLoopConfig, EventLoopRunSummary, JsRuntimeError, MediaMetrics,
    ResearchCanvasHost, ResearchCssomHost, ResearchDom, ResearchMediaHost, ResearchWebApiHost,
    ResearchWorkerHost, ScheduledVm, VmConfig, WebApiHost, WebApiMetrics, WebApiResponse,
    WorkerHost, WorkerMetrics,
};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use std::time::Instant;

/// Compatibility workload profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteCompatibilityProfile {
    /// Fast smoke profile for CI and local checks.
    Smoke,

    /// Default development profile.
    Standard,

    /// Heavier stress profile for regression studies.
    Stress,
}

impl Default for SiteCompatibilityProfile {
    fn default() -> Self {
        Self::Standard
    }
}

impl SiteCompatibilityProfile {
    /// Returns a lowercase stable id.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Standard => "standard",
            Self::Stress => "stress",
        }
    }
}

/// Named synthetic compatibility suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SiteCompatibilitySuite {
    /// Search portal workload with suggestions, results, URL/history, and timers.
    GoogleSearch,

    /// Repository page workload with API calls, file tree, issues, and worker messages.
    GitHubRepository,

    /// Article page workload with article layout, TOC, infobox, cookies, and references.
    WikipediaArticle,
}

impl SiteCompatibilitySuite {
    /// All suites in deterministic order.
    pub const ALL: [Self; 3] = [
        Self::GoogleSearch,
        Self::GitHubRepository,
        Self::WikipediaArticle,
    ];

    /// Stable suite id.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::GoogleSearch => "google-search-synthetic",
            Self::GitHubRepository => "github-repository-synthetic",
            Self::WikipediaArticle => "wikipedia-article-synthetic",
        }
    }

    /// Human-readable title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::GoogleSearch => "Google Search synthetic suite",
            Self::GitHubRepository => "GitHub Repository synthetic suite",
            Self::WikipediaArticle => "Wikipedia Article synthetic suite",
        }
    }

    /// Deterministic target URL.
    #[must_use]
    pub const fn target_url(self) -> &'static str {
        match self {
            Self::GoogleSearch => "https://compat.google.test/search?q=sylphos",
            Self::GitHubRepository => "https://compat.github.test/MaheshChandraTeja/Sylphos",
            Self::WikipediaArticle => "https://compat.wikipedia.test/wiki/Sylphos",
        }
    }

    /// Short description.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::GoogleSearch => "Search box, suggestion fetches, results rendering, URL/history state, storage, timers, and microtasks.",
            Self::GitHubRepository => "Repository header, file tree, code rows, issue cards, REST-like API fetches, worker diff analysis, and CSSOM mutations.",
            Self::WikipediaArticle => "Article shell, infobox, table of contents, section rendering, reference fetches, cookie/storage state, and periodic layout work.",
        }
    }
}

/// Signal measured by the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SiteCompatibilitySignal {
    /// VM instruction count.
    VmInstructions,

    /// Event-loop jobs executed.
    EventLoopJobs,

    /// Microtasks executed.
    Microtasks,

    /// Timers fired.
    Timers,

    /// DOM mutations.
    DomMutations,

    /// DOM queries.
    DomQueries,

    /// CSSOM mutations.
    CssomMutations,

    /// Computed style reads.
    ComputedStyleReads,

    /// Fetch calls.
    FetchCalls,

    /// Storage writes.
    StorageWrites,

    /// Cookie writes.
    CookieWrites,

    /// History mutations.
    HistoryMutations,

    /// URL helper objects.
    UrlObjects,

    /// Canvas commands.
    CanvasCommands,

    /// Media events.
    MediaEvents,

    /// Worker messages.
    WorkerMessages,

    /// Console lines emitted.
    ConsoleLines,
}

impl SiteCompatibilitySignal {
    /// Stable signal id.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VmInstructions => "vm_instructions",
            Self::EventLoopJobs => "event_loop_jobs",
            Self::Microtasks => "microtasks",
            Self::Timers => "timers",
            Self::DomMutations => "dom_mutations",
            Self::DomQueries => "dom_queries",
            Self::CssomMutations => "cssom_mutations",
            Self::ComputedStyleReads => "computed_style_reads",
            Self::FetchCalls => "fetch_calls",
            Self::StorageWrites => "storage_writes",
            Self::CookieWrites => "cookie_writes",
            Self::HistoryMutations => "history_mutations",
            Self::UrlObjects => "url_objects",
            Self::CanvasCommands => "canvas_commands",
            Self::MediaEvents => "media_events",
            Self::WorkerMessages => "worker_messages",
            Self::ConsoleLines => "console_lines",
        }
    }
}

/// Harness configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCompatibilityHarnessConfig {
    /// Profile used to size the synthetic workloads.
    pub profile: SiteCompatibilityProfile,

    /// Suites to run. Empty means all suites.
    pub suites: Vec<SiteCompatibilitySuite>,

    /// DOM/content loop size per suite.
    pub dom_nodes_per_suite: u32,

    /// Style mutation loop size.
    pub style_mutations_per_suite: u32,

    /// Worker messages per suite.
    pub worker_messages_per_suite: u32,

    /// Timer ticks per suite.
    pub timer_ticks_per_suite: u32,

    /// Canvas draw iterations per suite.
    pub canvas_draws_per_suite: u32,

    /// VM instruction budget.
    pub instruction_budget: u64,

    /// VM call depth.
    pub max_call_depth: usize,

    /// Event-loop job budget.
    pub max_jobs_per_run: u64,

    /// Whether the synthetic event loop should auto-advance timers.
    pub auto_advance_time: bool,
}

impl Default for SiteCompatibilityHarnessConfig {
    fn default() -> Self {
        Self::for_profile(SiteCompatibilityProfile::Standard)
    }
}

impl SiteCompatibilityHarnessConfig {
    /// Builds a profile-sized config.
    #[must_use]
    pub fn for_profile(profile: SiteCompatibilityProfile) -> Self {
        match profile {
            SiteCompatibilityProfile::Smoke => Self {
                profile,
                suites: SiteCompatibilitySuite::ALL.to_vec(),
                dom_nodes_per_suite: 4,
                style_mutations_per_suite: 3,
                worker_messages_per_suite: 2,
                timer_ticks_per_suite: 2,
                canvas_draws_per_suite: 2,
                instruction_budget: 250_000,
                max_call_depth: 128,
                max_jobs_per_run: 2_500,
                auto_advance_time: true,
            },
            SiteCompatibilityProfile::Standard => Self {
                profile,
                suites: SiteCompatibilitySuite::ALL.to_vec(),
                dom_nodes_per_suite: 12,
                style_mutations_per_suite: 8,
                worker_messages_per_suite: 4,
                timer_ticks_per_suite: 4,
                canvas_draws_per_suite: 4,
                instruction_budget: 1_250_000,
                max_call_depth: 256,
                max_jobs_per_run: 15_000,
                auto_advance_time: true,
            },
            SiteCompatibilityProfile::Stress => Self {
                profile,
                suites: SiteCompatibilitySuite::ALL.to_vec(),
                dom_nodes_per_suite: 36,
                style_mutations_per_suite: 24,
                worker_messages_per_suite: 12,
                timer_ticks_per_suite: 8,
                canvas_draws_per_suite: 12,
                instruction_budget: 5_000_000,
                max_call_depth: 512,
                max_jobs_per_run: 75_000,
                auto_advance_time: true,
            },
        }
    }

    /// Returns a sanitized config.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        if self.suites.is_empty() {
            self.suites = SiteCompatibilitySuite::ALL.to_vec();
        }

        self.suites.sort();
        self.suites.dedup();
        self.dom_nodes_per_suite = self.dom_nodes_per_suite.clamp(1, 5_000);
        self.style_mutations_per_suite = self.style_mutations_per_suite.clamp(0, 10_000);
        self.worker_messages_per_suite = self.worker_messages_per_suite.clamp(0, 10_000);
        self.timer_ticks_per_suite = self.timer_ticks_per_suite.clamp(0, 10_000);
        self.canvas_draws_per_suite = self.canvas_draws_per_suite.clamp(0, 10_000);
        self.instruction_budget = self.instruction_budget.max(10_000);
        self.max_call_depth = self.max_call_depth.max(32);
        self.max_jobs_per_run = self.max_jobs_per_run.max(128);
        self
    }

    /// VM configuration.
    #[must_use]
    pub fn vm_config(&self) -> VmConfig {
        VmConfig {
            instruction_budget: self.instruction_budget,
            max_call_depth: self.max_call_depth,
        }
    }

    /// Event-loop configuration.
    #[must_use]
    pub fn event_loop_config(&self) -> EventLoopConfig {
        EventLoopConfig {
            max_jobs_per_run: self.max_jobs_per_run,
            max_timer_advances: self.max_jobs_per_run,
            auto_advance_time: self.auto_advance_time,
        }
    }
}

/// JSON-friendly metric bundle for one suite.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCompatibilityMetrics {
    /// Wall-clock runtime in microseconds.
    pub wall_time_us: u128,

    /// Byte length of generated script.
    pub script_bytes: usize,

    /// Line count of generated script.
    pub script_lines: usize,

    /// VM instructions executed.
    pub vm_instructions: u64,

    /// Event-loop jobs executed.
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

    /// Computed style reads.
    pub computed_style_reads: u64,

    /// Fetch calls.
    pub fetch_calls: u64,

    /// XHR sends.
    pub xhr_sends: u64,

    /// Storage writes.
    pub storage_writes: u64,

    /// Cookie writes.
    pub cookie_writes: u64,

    /// History mutations.
    pub history_mutations: u64,

    /// URL helper objects created.
    pub url_objects: u64,

    /// Canvas commands recorded.
    pub canvas_commands: u64,

    /// Media events emitted.
    pub media_events: u64,

    /// Worker messages total.
    pub worker_messages: u64,

    /// Console lines.
    pub console_lines: usize,

    /// Whether the event loop hit a configured limit.
    pub hit_limit: bool,
}

impl SiteCompatibilityMetrics {
    fn value_for(&self, signal: SiteCompatibilitySignal) -> u64 {
        match signal {
            SiteCompatibilitySignal::VmInstructions => self.vm_instructions,
            SiteCompatibilitySignal::EventLoopJobs => self.event_loop_jobs,
            SiteCompatibilitySignal::Microtasks => self.microtasks_executed,
            SiteCompatibilitySignal::Timers => self.timers_fired,
            SiteCompatibilitySignal::DomMutations => self.dom_mutations,
            SiteCompatibilitySignal::DomQueries => self.dom_queries,
            SiteCompatibilitySignal::CssomMutations => self.cssom_mutations,
            SiteCompatibilitySignal::ComputedStyleReads => self.computed_style_reads,
            SiteCompatibilitySignal::FetchCalls => self.fetch_calls,
            SiteCompatibilitySignal::StorageWrites => self.storage_writes,
            SiteCompatibilitySignal::CookieWrites => self.cookie_writes,
            SiteCompatibilitySignal::HistoryMutations => self.history_mutations,
            SiteCompatibilitySignal::UrlObjects => self.url_objects,
            SiteCompatibilitySignal::CanvasCommands => self.canvas_commands,
            SiteCompatibilitySignal::MediaEvents => self.media_events,
            SiteCompatibilitySignal::WorkerMessages => self.worker_messages,
            SiteCompatibilitySignal::ConsoleLines => self.console_lines as u64,
        }
    }

    /// Returns a compact human-readable metric line.
    #[must_use]
    pub fn compact(&self) -> String {
        format!(
            "instr={} jobs={} dom={} cssom={} fetch={} storage={} history={} worker={} canvas={} console={} limit={}",
            self.vm_instructions,
            self.event_loop_jobs,
            self.dom_mutations,
            self.cssom_mutations,
            self.fetch_calls,
            self.storage_writes,
            self.history_mutations,
            self.worker_messages,
            self.canvas_commands,
            self.console_lines,
            self.hit_limit,
        )
    }
}

/// One scored compatibility signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCompatibilityScore {
    /// Signal.
    pub signal: SiteCompatibilitySignal,

    /// Minimum expected value.
    pub minimum: u64,

    /// Actual value measured.
    pub actual: u64,

    /// Signal weight.
    pub weight: u32,

    /// Whether actual reached the minimum.
    pub passed: bool,
}

/// One synthetic suite result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCompatibilitySuiteRun {
    /// Suite id.
    pub suite: SiteCompatibilitySuite,

    /// Stable target URL used by the suite.
    pub target_url: String,

    /// Suite title.
    pub title: String,

    /// Suite description.
    pub description: String,

    /// Metrics.
    pub metrics: SiteCompatibilityMetrics,

    /// Signal scores.
    pub scores: Vec<SiteCompatibilityScore>,

    /// Weighted pass percentage.
    pub score_percent: u8,

    /// Overall pass/fail.
    pub passed: bool,

    /// Captured console lines.
    pub console: Vec<String>,

    /// Warnings generated by the harness.
    pub warnings: Vec<String>,
}

/// Aggregate compatibility report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCompatibilityAggregate {
    /// Suite count.
    pub suites: usize,

    /// Passed suite count.
    pub passed: usize,

    /// Failed suite count.
    pub failed: usize,

    /// Average weighted score.
    pub average_score_percent: u8,

    /// Total VM instructions.
    pub total_vm_instructions: u64,

    /// Total DOM mutations.
    pub total_dom_mutations: u64,

    /// Total CSSOM mutations.
    pub total_cssom_mutations: u64,

    /// Total fetch calls.
    pub total_fetch_calls: u64,

    /// Total worker messages.
    pub total_worker_messages: u64,

    /// Total canvas commands.
    pub total_canvas_commands: u64,

    /// Whether any suite hit a runtime/event-loop limit.
    pub any_hit_limit: bool,
}

/// Full harness run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteCompatibilityHarnessRun {
    /// Config used.
    pub config: SiteCompatibilityHarnessConfig,

    /// Aggregate summary.
    pub aggregate: SiteCompatibilityAggregate,

    /// Per-suite runs.
    pub suites: Vec<SiteCompatibilitySuiteRun>,
}

/// Runs the deterministic synthetic site compatibility harness.
pub fn run_site_compatibility_harness(
    config: SiteCompatibilityHarnessConfig,
) -> Result<SiteCompatibilityHarnessRun, JsRuntimeError> {
    let config = config.sanitized();
    let mut suite_runs = Vec::with_capacity(config.suites.len());

    for suite in config.suites.iter().copied() {
        suite_runs.push(run_site_compatibility_suite(suite, &config)?);
    }

    let aggregate = aggregate_runs(&suite_runs);

    Ok(SiteCompatibilityHarnessRun {
        config,
        aggregate,
        suites: suite_runs,
    })
}

/// Builds the generated JavaScript source for a suite.
#[must_use]
pub fn build_site_compatibility_script(
    suite: SiteCompatibilitySuite,
    config: &SiteCompatibilityHarnessConfig,
) -> String {
    match suite {
        SiteCompatibilitySuite::GoogleSearch => build_google_search_script(config),
        SiteCompatibilitySuite::GitHubRepository => build_github_repository_script(config),
        SiteCompatibilitySuite::WikipediaArticle => build_wikipedia_article_script(config),
    }
}

fn run_site_compatibility_suite(
    suite: SiteCompatibilitySuite,
    config: &SiteCompatibilityHarnessConfig,
) -> Result<SiteCompatibilitySuiteRun, JsRuntimeError> {
    let started = Instant::now();
    let cssom = Rc::new(ResearchCssomHost::default());
    let dom = Rc::new(ResearchDom::with_cssom(suite.title(), cssom.clone()));
    let web = Rc::new(ResearchWebApiHost::new(suite.target_url()));
    let media = Rc::new(ResearchMediaHost::default());
    let canvas = Rc::new(ResearchCanvasHost::default());
    let workers = Rc::new(ResearchWorkerHost::default());

    register_suite_routes(&web, suite, config);
    register_suite_workers(&workers, suite);

    let mut scheduled = ScheduledVm::with_config(config.vm_config(), config.event_loop_config());

    install_dom_globals(&mut scheduled.vm, scheduled.event_loop.clone(), dom.clone());
    install_cssom_globals(
        &mut scheduled.vm,
        scheduled.event_loop.clone(),
        dom.clone(),
        cssom.clone(),
    );
    install_web_api_globals(&mut scheduled.vm, scheduled.event_loop.clone(), web.clone());
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

    let script = build_site_compatibility_script(suite, config);
    let script_bytes = script.len();
    let script_lines = script.lines().count();
    let program = parse_script(&script).map_err(JsRuntimeError::from_frontend_error)?;
    let bytecode = compile_program(&program, Default::default())?;
    scheduled.vm.execute(&bytecode)?;
    let summary = scheduled.run_until_idle()?;

    let metrics = metrics_from_hosts(
        started.elapsed().as_micros(),
        script_bytes,
        script_lines,
        &summary,
        dom.metrics(),
        cssom.metrics(),
        web.metrics(),
        media.metrics(),
        canvas.metrics(),
        workers.metrics(),
    );
    let scores = evaluate_suite(suite, config, &metrics);
    let score_percent = weighted_score_percent(&scores);
    let passed = score_percent >= 80 && !metrics.hit_limit;
    let mut warnings = Vec::new();

    if metrics.hit_limit {
        warnings.push(
            "suite hit VM or event-loop limit; raise budgets or inspect runaway work".to_owned(),
        );
    }

    for score in &scores {
        if !score.passed {
            warnings.push(format!(
                "signal `{}` below minimum: actual={} minimum={}",
                score.signal.as_str(),
                score.actual,
                score.minimum
            ));
        }
    }

    Ok(SiteCompatibilitySuiteRun {
        suite,
        target_url: suite.target_url().to_owned(),
        title: suite.title().to_owned(),
        description: suite.description().to_owned(),
        metrics,
        scores,
        score_percent,
        passed,
        console: summary.console.clone(),
        warnings,
    })
}

#[allow(clippy::too_many_arguments)]
fn metrics_from_hosts(
    wall_time_us: u128,
    script_bytes: usize,
    script_lines: usize,
    summary: &EventLoopRunSummary,
    dom: DomBindingMetrics,
    cssom: CssomMetrics,
    web: WebApiMetrics,
    media: MediaMetrics,
    canvas: CanvasMetrics,
    workers: WorkerMetrics,
) -> SiteCompatibilityMetrics {
    let dom_mutations = dom
        .text_mutations
        .saturating_add(dom.attribute_mutations)
        .saturating_add(dom.value_mutations)
        .saturating_add(dom.structure_mutations);
    let cssom_mutations = cssom
        .inline_writes
        .saturating_add(cssom.inline_removals)
        .saturating_add(cssom.rules_inserted)
        .saturating_add(cssom.rules_deleted);
    let history_mutations = web.history_pushes.saturating_add(web.history_replaces);
    let url_objects = web
        .urls_created
        .saturating_add(web.url_search_params_created);
    let worker_messages = workers
        .messages_to_worker
        .saturating_add(workers.messages_to_main);

    SiteCompatibilityMetrics {
        wall_time_us,
        script_bytes,
        script_lines,
        vm_instructions: summary.vm.instructions_executed,
        event_loop_jobs: summary.jobs_executed,
        microtasks_executed: summary.event_loop.microtasks_executed,
        promise_reactions_executed: summary.event_loop.promise_reactions_executed,
        timers_fired: summary.event_loop.timers_fired,
        dom_mutations,
        dom_queries: dom.queries,
        cssom_mutations,
        computed_style_reads: cssom.computed_reads,
        fetch_calls: web.fetch_calls,
        xhr_sends: web.xhr_sends,
        storage_writes: web.storage_writes,
        cookie_writes: web.cookie_writes,
        history_mutations,
        url_objects,
        canvas_commands: canvas.commands_recorded,
        media_events: media.events,
        worker_messages,
        console_lines: summary.console.len(),
        hit_limit: summary.hit_limit,
    }
}

fn evaluate_suite(
    suite: SiteCompatibilitySuite,
    config: &SiteCompatibilityHarnessConfig,
    metrics: &SiteCompatibilityMetrics,
) -> Vec<SiteCompatibilityScore> {
    expectations_for_suite(suite, config)
        .into_iter()
        .map(|expectation| {
            let actual = metrics.value_for(expectation.signal);
            SiteCompatibilityScore {
                signal: expectation.signal,
                minimum: expectation.minimum,
                actual,
                weight: expectation.weight,
                passed: actual >= expectation.minimum,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct Expectation {
    signal: SiteCompatibilitySignal,
    minimum: u64,
    weight: u32,
}

const fn expectation(signal: SiteCompatibilitySignal, minimum: u64, weight: u32) -> Expectation {
    Expectation {
        signal,
        minimum,
        weight,
    }
}

fn expectations_for_suite(
    suite: SiteCompatibilitySuite,
    config: &SiteCompatibilityHarnessConfig,
) -> Vec<Expectation> {
    let dom_min = u64::from(config.dom_nodes_per_suite).max(1);
    let style_min = u64::from(config.style_mutations_per_suite.min(3)).max(1);
    let worker_min = u64::from(config.worker_messages_per_suite).max(1);
    let timer_min = u64::from(config.timer_ticks_per_suite).min(3).max(1);
    let canvas_min = u64::from(config.canvas_draws_per_suite).min(3).max(1);

    match suite {
        SiteCompatibilitySuite::GoogleSearch => vec![
            expectation(SiteCompatibilitySignal::VmInstructions, 100, 2),
            expectation(SiteCompatibilitySignal::DomMutations, dom_min, 3),
            expectation(SiteCompatibilitySignal::CssomMutations, style_min, 2),
            expectation(SiteCompatibilitySignal::FetchCalls, 2, 3),
            expectation(SiteCompatibilitySignal::StorageWrites, 1, 2),
            expectation(SiteCompatibilitySignal::HistoryMutations, 1, 2),
            expectation(SiteCompatibilitySignal::Timers, timer_min, 1),
            expectation(SiteCompatibilitySignal::Microtasks, 1, 1),
            expectation(SiteCompatibilitySignal::ConsoleLines, 3, 1),
        ],
        SiteCompatibilitySuite::GitHubRepository => vec![
            expectation(SiteCompatibilitySignal::VmInstructions, 100, 2),
            expectation(SiteCompatibilitySignal::DomMutations, dom_min, 3),
            expectation(SiteCompatibilitySignal::DomQueries, 1, 1),
            expectation(SiteCompatibilitySignal::CssomMutations, style_min, 2),
            expectation(SiteCompatibilitySignal::FetchCalls, 3, 3),
            expectation(SiteCompatibilitySignal::StorageWrites, 1, 1),
            expectation(SiteCompatibilitySignal::HistoryMutations, 1, 1),
            expectation(SiteCompatibilitySignal::WorkerMessages, worker_min, 2),
            expectation(SiteCompatibilitySignal::ConsoleLines, 3, 1),
        ],
        SiteCompatibilitySuite::WikipediaArticle => vec![
            expectation(SiteCompatibilitySignal::VmInstructions, 100, 2),
            expectation(SiteCompatibilitySignal::DomMutations, dom_min, 3),
            expectation(SiteCompatibilitySignal::CssomMutations, style_min, 2),
            expectation(SiteCompatibilitySignal::FetchCalls, 2, 3),
            expectation(SiteCompatibilitySignal::StorageWrites, 1, 1),
            expectation(SiteCompatibilitySignal::CookieWrites, 1, 1),
            expectation(SiteCompatibilitySignal::HistoryMutations, 1, 1),
            expectation(SiteCompatibilitySignal::CanvasCommands, canvas_min, 1),
            expectation(SiteCompatibilitySignal::Timers, timer_min, 1),
            expectation(SiteCompatibilitySignal::ConsoleLines, 3, 1),
        ],
    }
}

fn weighted_score_percent(scores: &[SiteCompatibilityScore]) -> u8 {
    let total_weight = scores.iter().map(|score| score.weight).sum::<u32>().max(1);
    let passed_weight = scores
        .iter()
        .filter(|score| score.passed)
        .map(|score| score.weight)
        .sum::<u32>();
    ((passed_weight * 100) / total_weight).min(100) as u8
}

fn aggregate_runs(runs: &[SiteCompatibilitySuiteRun]) -> SiteCompatibilityAggregate {
    if runs.is_empty() {
        return SiteCompatibilityAggregate::default();
    }

    let passed = runs.iter().filter(|run| run.passed).count();
    let score_sum = runs
        .iter()
        .map(|run| u32::from(run.score_percent))
        .sum::<u32>();

    SiteCompatibilityAggregate {
        suites: runs.len(),
        passed,
        failed: runs.len().saturating_sub(passed),
        average_score_percent: (score_sum / runs.len() as u32).min(100) as u8,
        total_vm_instructions: runs.iter().map(|run| run.metrics.vm_instructions).sum(),
        total_dom_mutations: runs.iter().map(|run| run.metrics.dom_mutations).sum(),
        total_cssom_mutations: runs.iter().map(|run| run.metrics.cssom_mutations).sum(),
        total_fetch_calls: runs.iter().map(|run| run.metrics.fetch_calls).sum(),
        total_worker_messages: runs.iter().map(|run| run.metrics.worker_messages).sum(),
        total_canvas_commands: runs.iter().map(|run| run.metrics.canvas_commands).sum(),
        any_hit_limit: runs.iter().any(|run| run.metrics.hit_limit),
    }
}

fn register_suite_routes(
    host: &ResearchWebApiHost,
    suite: SiteCompatibilitySuite,
    config: &SiteCompatibilityHarnessConfig,
) {
    match suite {
        SiteCompatibilitySuite::GoogleSearch => {
            host.register_route(
                "/complete/search?q=sylphos",
                WebApiResponse::text(
                    "https://compat.google.test/complete/search?q=sylphos",
                    "sylphos,sylphos browser,sylphos github,sylphos wikipedia",
                ),
            );
            host.register_route(
                "/search?q=sylphos",
                WebApiResponse::text(
                    "https://compat.google.test/search?q=sylphos",
                    format!("results:{}", config.dom_nodes_per_suite),
                ),
            );
        }
        SiteCompatibilitySuite::GitHubRepository => {
            host.register_route(
                "/api/repos/sylphos",
                WebApiResponse::text(
                    "https://compat.github.test/api/repos/sylphos",
                    "repo:Sylphos;stars:45;language:Rust;license:MIT OR Apache-2.0",
                ),
            );
            host.register_route(
                "/api/repos/sylphos/files",
                WebApiResponse::text(
                    "https://compat.github.test/api/repos/sylphos/files",
                    "Cargo.toml,crates/syljs/src/lib.rs,crates/app/src/main.rs,README.md",
                ),
            );
            host.register_route(
                "/api/repos/sylphos/issues",
                WebApiResponse::text(
                    "https://compat.github.test/api/repos/sylphos/issues",
                    "#1 compatibility harness;#2 service worker cache;#3 SVG icons",
                ),
            );
        }
        SiteCompatibilitySuite::WikipediaArticle => {
            host.register_route(
                "/api/rest_v1/page/summary/Sylphos",
                WebApiResponse::text(
                    "https://compat.wikipedia.test/api/rest_v1/page/summary/Sylphos",
                    "Sylphos is a synthetic browser engine used for deterministic compatibility research.",
                ),
            );
            host.register_route(
                "/w/api.php?action=parse&page=Sylphos&prop=sections",
                WebApiResponse::text(
                    "https://compat.wikipedia.test/w/api.php?action=parse&page=Sylphos&prop=sections",
                    "History|Architecture|Rendering|JavaScript|Compatibility",
                ),
            );
        }
    }
}

fn register_suite_workers(host: &ResearchWorkerHost, suite: SiteCompatibilitySuite) {
    match suite {
        SiteCompatibilitySuite::GoogleSearch => host.register_script(
            "google-ranking-worker.js",
            r#"
                self.onmessage = function (event) {
                    postMessage("ranked:" + event.data);
                };
            "#,
        ),
        SiteCompatibilitySuite::GitHubRepository => host.register_script(
            "github-diff-worker.js",
            r#"
                self.onmessage = function (event) {
                    postMessage("diff-ready:" + event.data);
                };
            "#,
        ),
        SiteCompatibilitySuite::WikipediaArticle => host.register_script(
            "wiki-reference-worker.js",
            r#"
                self.onmessage = function (event) {
                    postMessage("reference-ready:" + event.data);
                };
            "#,
        ),
    }
}

fn build_google_search_script(config: &SiteCompatibilityHarnessConfig) -> String {
    format!(
        r##"
document.title = "Google Synthetic Search";
history.replaceState({{}}, "", "/search?q=sylphos");
localStorage.setItem("google.query", "sylphos");
sessionStorage.setItem("google.session", "compat");

document.styleSheets[0].insertRule(".search-shell {{ color: #202124; background-color: #ffffff; }}", 0);
document.styleSheets[0].insertRule(".result-card {{ color: #1a0dab; font-size: 14px; }}", 1);
document.styleSheets[0].insertRule(".suggestion {{ color: #3c4043; }}", 2);

const shell = document.createElement("main");
shell.id = "search-shell";
shell.className = "search-shell";
document.body.appendChild(shell);

const logo = document.createElement("h1");
logo.textContent = "Sylphos Search";
shell.appendChild(logo);

const form = document.createElement("form");
form.id = "search-form";
shell.appendChild(form);

const input = document.createElement("input");
input.id = "q";
input.value = "sylphos";
input.setAttribute("aria-label", "Search");
form.appendChild(input);

const button = document.createElement("button");
button.textContent = "Search";
form.appendChild(button);

const suggestions = document.createElement("section");
suggestions.id = "suggestions";
shell.appendChild(suggestions);

const results = document.createElement("section");
results.id = "results";
shell.appendChild(results);

let resultIndex = 0;
while (resultIndex < {dom_nodes}) {{
    const card = document.createElement("article");
    card.className = "result-card";
    card.textContent = "Synthetic result " + resultIndex + " for Sylphos";
    results.appendChild(card);
    resultIndex = resultIndex + 1;
}}

const worker = new Worker("google-ranking-worker.js");
worker.onmessage = function (event) {{
    console.log("google-worker", event.data);
}};
worker.postMessage("sylphos");

fetch("/complete/search?q=sylphos").then(function (response) {{
    response.text().then(function (text) {{
        const item = document.createElement("p");
        item.className = "suggestion";
        item.textContent = text;
        suggestions.appendChild(item);
        console.log("suggestions", text);
    }});
}});

fetch("/search?q=sylphos").then(function (response) {{
    response.text().then(function (text) {{
        localStorage.setItem("google.results", text);
        console.log("results", text);
    }});
}});

let styleIndex = 0;
while (styleIndex < {style_mutations}) {{
    logo.style.fontSize = (24 + styleIndex) + "px";
    logo.style.color = "#202124";
    const computed = getComputedStyle(logo).color;
    if (styleIndex == 0) {{
        console.log("google-computed", computed);
    }}
    styleIndex = styleIndex + 1;
}}

let tick = 0;
const timer = setInterval(function () {{
    tick = tick + 1;
    input.value = "sylphos " + tick;
    if (tick >= {timer_ticks}) {{
        clearInterval(timer);
        console.log("google-timers", tick);
    }}
}}, 4);

queueMicrotask(function () {{
    console.log("google-microtask", document.title);
}});

console.log("google-suite-start", document.title);
"##,
        dom_nodes = config.dom_nodes_per_suite,
        style_mutations = config.style_mutations_per_suite,
        timer_ticks = config.timer_ticks_per_suite.max(1),
    )
}

fn build_github_repository_script(config: &SiteCompatibilityHarnessConfig) -> String {
    format!(
        r##"
document.title = "GitHub Synthetic Repository";
history.pushState({{}}, "", "/MaheshChandraTeja/Sylphos");
localStorage.setItem("github.repo", "MaheshChandraTeja/Sylphos");

document.styleSheets[0].insertRule(".repo-shell {{ color: #24292f; background-color: #ffffff; }}", 0);
document.styleSheets[0].insertRule(".file-row {{ font-family: monospace; font-size: 13px; }}", 1);
document.styleSheets[0].insertRule(".issue-card {{ border: 1px solid #d0d7de; }}", 2);

const shell = document.createElement("section");
shell.id = "repo-shell";
shell.className = "repo-shell";
document.body.appendChild(shell);

const header = document.createElement("h1");
header.textContent = "MaheshChandraTeja / Sylphos";
shell.appendChild(header);

const tabs = document.createElement("nav");
tabs.id = "repo-tabs";
tabs.textContent = "Code Issues Pull requests Actions";
shell.appendChild(tabs);

const fileTree = document.createElement("section");
fileTree.id = "file-tree";
shell.appendChild(fileTree);

let fileIndex = 0;
while (fileIndex < {dom_nodes}) {{
    const row = document.createElement("div");
    row.className = "file-row";
    row.textContent = "crates/module_" + fileIndex + ".rs";
    fileTree.appendChild(row);
    fileIndex = fileIndex + 1;
}}

const code = document.createElement("pre");
code.id = "code-view";
code.textContent = "pub fn module_45() {{ /* synthetic compatibility */ }}";
shell.appendChild(code);

const selected = document.querySelector("#code-view");
if (selected) {{
    selected.setAttribute("data-selected", "true");
}}

const params = new URLSearchParams("tab=code&suite=compat");
localStorage.setItem("github.tab", params.get("tab"));

fetch("/api/repos/sylphos").then(function (response) {{
    response.text().then(function (text) {{
        header.textContent = text;
        console.log("repo", text);
    }});
}});

fetch("/api/repos/sylphos/files").then(function (response) {{
    response.text().then(function (text) {{
        sessionStorage.setItem("github.files", text);
        console.log("files", text);
    }});
}});

fetch("/api/repos/sylphos/issues").then(function (response) {{
    response.text().then(function (text) {{
        const issue = document.createElement("article");
        issue.className = "issue-card";
        issue.textContent = text;
        shell.appendChild(issue);
        console.log("issues", text);
    }});
}});

const worker = new Worker("github-diff-worker.js");
worker.onmessage = function (event) {{
    console.log("github-worker", event.data);
}};

let workerIndex = 0;
while (workerIndex < {worker_messages}) {{
    worker.postMessage("diff-" + workerIndex);
    workerIndex = workerIndex + 1;
}}

let styleIndex = 0;
while (styleIndex < {style_mutations}) {{
    code.style.fontSize = (12 + styleIndex) + "px";
    const computed = getComputedStyle(code).fontSize;
    if (styleIndex == 0) {{
        console.log("github-computed", computed);
    }}
    styleIndex = styleIndex + 1;
}}

queueMicrotask(function () {{
    console.log("github-microtask", document.title);
}});

console.log("github-suite-start", document.title);
"##,
        dom_nodes = config.dom_nodes_per_suite,
        worker_messages = config.worker_messages_per_suite,
        style_mutations = config.style_mutations_per_suite,
    )
}

fn build_wikipedia_article_script(config: &SiteCompatibilityHarnessConfig) -> String {
    format!(
        r##"
document.title = "Wikipedia Synthetic Article";
history.replaceState({{}}, "", "/wiki/Sylphos");
sessionStorage.setItem("wiki.article", "Sylphos");
document.cookie = "wikiSession=compat; path=/";

document.styleSheets[0].insertRule(".mw-parser-output {{ color: #202122; }}", 0);
document.styleSheets[0].insertRule(".infobox {{ float: right; border: 1px solid #a2a9b1; }}", 1);
document.styleSheets[0].insertRule(".toc {{ border: 1px solid #a2a9b1; }}", 2);

const article = document.createElement("article");
article.id = "mw-content-text";
article.className = "mw-parser-output";
document.body.appendChild(article);

const heading = document.createElement("h1");
heading.textContent = "Sylphos";
article.appendChild(heading);

const infobox = document.createElement("aside");
infobox.className = "infobox";
infobox.textContent = "Synthetic browser engine";
article.appendChild(infobox);

const toc = document.createElement("nav");
toc.className = "toc";
toc.textContent = "Contents";
article.appendChild(toc);

let sectionIndex = 0;
while (sectionIndex < {dom_nodes}) {{
    const section = document.createElement("section");
    section.className = "mw-section";
    const h2 = document.createElement("h2");
    h2.textContent = "Section " + sectionIndex;
    const p = document.createElement("p");
    p.textContent = "Synthetic encyclopedic paragraph " + sectionIndex;
    section.appendChild(h2);
    section.appendChild(p);
    article.appendChild(section);
    sectionIndex = sectionIndex + 1;
}}

const canvas = new HTMLCanvasElement();
canvas.width = 320;
canvas.height = 120;
article.appendChild(canvas);
const ctx = canvas.getContext("2d");

let draw = 0;
while (draw < {canvas_draws}) {{
    ctx.fillStyle = "#f8f9fa";
    ctx.fillRect(0, 0, 320, 120);
    ctx.fillStyle = "#202122";
    ctx.fillText("Wiki draw " + draw, 12, 24 + draw);
    draw = draw + 1;
}}

fetch("/api/rest_v1/page/summary/Sylphos").then(function (response) {{
    response.text().then(function (text) {{
        const summary = document.createElement("p");
        summary.textContent = text;
        article.appendChild(summary);
        localStorage.setItem("wiki.summary", text);
        console.log("summary", text);
    }});
}});

fetch("/w/api.php?action=parse&page=Sylphos&prop=sections").then(function (response) {{
    response.text().then(function (text) {{
        toc.textContent = text;
        console.log("sections", text);
    }});
}});

const worker = new Worker("wiki-reference-worker.js");
worker.onmessage = function (event) {{
    console.log("wiki-worker", event.data);
}};
worker.postMessage("references");

let styleIndex = 0;
while (styleIndex < {style_mutations}) {{
    heading.style.fontSize = (28 + styleIndex) + "px";
    const computed = getComputedStyle(heading).fontSize;
    if (styleIndex == 0) {{
        console.log("wiki-computed", computed);
    }}
    styleIndex = styleIndex + 1;
}}

let tick = 0;
const timer = setInterval(function () {{
    tick = tick + 1;
    infobox.textContent = "Synthetic browser engine " + tick;
    if (tick >= {timer_ticks}) {{
        clearInterval(timer);
        console.log("wiki-timers", tick);
    }}
}}, 5);

queueMicrotask(function () {{
    console.log("wiki-microtask", document.title);
}});

console.log("wikipedia-suite-start", document.title);
"##,
        dom_nodes = config.dom_nodes_per_suite,
        canvas_draws = config.canvas_draws_per_suite,
        style_mutations = config.style_mutations_per_suite,
        timer_ticks = config.timer_ticks_per_suite.max(1),
    )
}
