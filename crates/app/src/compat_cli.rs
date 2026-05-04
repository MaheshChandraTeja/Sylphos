//! CLI helpers for the Sylphos site compatibility harness.

use crate::js::{run_app_site_compatibility_harness, AppSiteCompatibilityRequest};
use anyhow::{bail, Context, Result};
use std::{fs, path::Path};
use syljs::{SiteCompatibilityProfile, SiteCompatibilitySuite};

/// Runs the compatibility harness from the app CLI.
pub(crate) fn run_site_compatibility_cli(
    profile: &str,
    suites: &[String],
    json_output: Option<&Path>,
) -> Result<()> {
    let profile = parse_profile(profile)?;
    let suites = parse_suites(suites)?;

    let response = run_app_site_compatibility_harness(AppSiteCompatibilityRequest {
        profile,
        suites,
        ..Default::default()
    })
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    println!("Sylphos Site Compatibility Harness");
    println!("{}", response.compact());

    for suite in &response.suites {
        println!(
            "- {} | score={} passed={} | {}",
            suite.suite.id(),
            suite.score_percent,
            suite.passed,
            suite.metrics.compact()
        );

        for warning in &suite.warnings {
            println!("  warning: {warning}");
        }
    }

    if let Some(path) = json_output {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create `{}`", parent.display()))?;
            }
        }

        let json = serde_json::to_string_pretty(&response.raw)
            .context("failed to serialize compatibility report")?;
        fs::write(path, json).with_context(|| format!("failed to write `{}`", path.display()))?;
        println!("wrote JSON report: {}", path.display());
    }

    Ok(())
}

fn parse_profile(input: &str) -> Result<SiteCompatibilityProfile> {
    match input.trim().to_ascii_lowercase().as_str() {
        "smoke" => Ok(SiteCompatibilityProfile::Smoke),
        "standard" | "default" => Ok(SiteCompatibilityProfile::Standard),
        "stress" => Ok(SiteCompatibilityProfile::Stress),
        other => bail!("unknown compatibility profile `{other}`; use smoke, standard, or stress"),
    }
}

fn parse_suites(inputs: &[String]) -> Result<Vec<SiteCompatibilitySuite>> {
    let mut suites = Vec::new();

    for input in inputs {
        for part in input.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
                continue;
            }
            suites.push(parse_suite(trimmed)?);
        }
    }

    suites.sort();
    suites.dedup();
    Ok(suites)
}

fn parse_suite(input: &str) -> Result<SiteCompatibilitySuite> {
    match input.trim().to_ascii_lowercase().as_str() {
        "google" | "google-search" | "search" => Ok(SiteCompatibilitySuite::GoogleSearch),
        "github" | "github-repository" | "repo" => Ok(SiteCompatibilitySuite::GitHubRepository),
        "wikipedia" | "wiki" | "article" => Ok(SiteCompatibilitySuite::WikipediaArticle),
        other => bail!("unknown compatibility suite `{other}`; use google, github, wikipedia, or all"),
    }
}
