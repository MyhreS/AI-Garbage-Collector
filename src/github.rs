use crate::runtime::{command_at, git};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::path::Path;

/// Check the worktree branch against open PRs in its GitHub repository and its
/// parent repository when the checkout is a fork. Detached checkouts use commit
/// associations. Errors remain distinct from a verified absence of open PRs.
pub fn open_pr(path: &Path) -> Result<Option<String>> {
    let reference = git(path, &["rev-parse", "--symbolic-full-name", "HEAD"])?;
    let branch = reference.trim().strip_prefix("refs/heads/");
    let head = git(path, &["rev-parse", "--verify", "HEAD"])?;
    ensure!(!head.trim().is_empty(), "worktree HEAD is unknown");

    let repositories = repositories(path)?;
    open_pr_for_ref(path, &repositories, branch, head.trim())
}

pub fn repositories(path: &Path) -> Result<Vec<String>> {
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
    let mut repositories = vec![name.to_owned()];
    if let Some(parent) = repo["parent"]["nameWithOwner"].as_str()
        && parent != name
    {
        repositories.push(parent.to_owned());
    }
    Ok(repositories)
}

pub fn open_pr_for_ref(
    path: &Path,
    repositories: &[String],
    branch: Option<&str>,
    head: &str,
) -> Result<Option<String>> {
    for repository in repositories {
        if let Some(branch) = branch {
            let output = command_at(
                "gh",
                &[
                    "pr", "list", "--repo", repository, "--state", "open", "--head", branch,
                    "--limit", "1", "--json", "url",
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
        } else {
            let endpoint = format!(
                "repos/{repository}/commits/{}/pulls?per_page=100",
                head.trim()
            );
            let output = command_at(
                "gh",
                &["api", &endpoint, "--paginate", "--slurp"],
                Some(path),
                30,
            )?;
            let pages: Value =
                serde_json::from_str(&output).context("invalid commit PR response")?;
            for page in pages.as_array().context("commit PR pages are not a list")? {
                for pr in page.as_array().context("commit PR page is not a list")? {
                    if pr["state"].as_str().context("missing PR state")? == "open" {
                        return Ok(Some(
                            pr["html_url"]
                                .as_str()
                                .context("missing PR URL")?
                                .to_owned(),
                        ));
                    }
                }
            }
        }
    }
    Ok(None)
}

pub fn ensure_no_open_pr(path: &Path, require_verification: bool) -> Result<()> {
    match open_pr(path) {
        Ok(Some(url)) => bail!("worktree has an open GitHub pull request: {url}"),
        Ok(None) => Ok(()),
        Err(e) if require_verification => {
            Err(e).context("could not verify GitHub pull requests; worktree retained")
        }
        Err(_) => Ok(()),
    }
}
