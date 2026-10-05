use crate::runtime::{command_at, git};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::path::Path;

/// Check the worktree branch against open PRs in its GitHub repository and its
/// parent repository when the checkout is a fork. An unavailable or ambiguous
/// check is an error so callers keep the worktree.
pub fn open_pr(path: &Path) -> Result<Option<String>> {
    let branch = git(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let branch = branch.trim();
    ensure!(!branch.is_empty(), "worktree branch is unknown");

    let repo: Value = serde_json::from_str(&command_at(
        "gh",
        &["repo", "view", "--json", "nameWithOwner,parent"],
        Some(path),
        30,
    )?)
    .context("invalid GitHub repository response")?;
    let name = repo["nameWithOwner"]
        .as_str()
        .context("GitHub repository identity is unavailable")?;
    let mut repositories = vec![name];
    if let Some(parent) = repo["parent"]["nameWithOwner"].as_str()
        && parent != name
    {
        repositories.push(parent);
    }
    for repository in repositories {
        let output = command_at(
            "gh",
            &[
                "pr", "list", "--repo", repository, "--state", "open", "--head", branch, "--limit",
                "1", "--json", "url",
            ],
            Some(path),
            30,
        )?;
        let prs: Value = serde_json::from_str(&output).context("invalid GitHub PR response")?;
        let prs = prs.as_array().context("GitHub PR response is not a list")?;
        if let Some(pr) = prs.first() {
            return Ok(Some(
                pr["url"]
                    .as_str()
                    .context("GitHub PR URL is unavailable")?
                    .to_owned(),
            ));
        }
    }
    Ok(None)
}

pub fn ensure_no_open_pr(path: &Path) -> Result<()> {
    match open_pr(path).context("could not verify GitHub pull requests; worktree retained")? {
        Some(url) => bail!("worktree has an open GitHub pull request: {url}"),
        None => Ok(()),
    }
}
