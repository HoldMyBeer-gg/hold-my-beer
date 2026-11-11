//! `collab join` and `collab pair`: add a machine to a team with a six-digit
//! code, from a terminal.
//!
//! `collab pair` runs on a machine that is already in the team and prints a
//! code. `collab join <address> <code>` runs on the new machine, trades the
//! code for a team token of its own, and saves it. Text handling lives in
//! [`crate::join_text`]; this module talks to the server and the disk.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::join_text::{display_code, merge_config, normalize_code, normalize_server};

/// What the server hands a machine that redeemed a pairing code.
#[derive(Debug, Deserialize)]
pub struct Joined {
    pub token: String,
    #[serde(default)]
    pub team_name: String,
}

/// A pairing code and how long it lasts.
#[derive(Debug, Deserialize)]
pub struct PairCode {
    pub code: String,
    pub expires_in: u64,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Trade a pairing code for a team token. Needs no credentials: the code is
/// the credential.
pub async fn redeem(server: &str, code: &str) -> Result<Joined> {
    let server = normalize_server(server);
    let code = normalize_code(code)
        .ok_or_else(|| anyhow!("A pairing code is six digits, for example 482 913."))?;

    let resp = http()
        .post(format!("{server}/pair/redeem"))
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Can't reach {server}. Check the address, that the server is running there, \
                 and that this machine can reach it. ({e})"
            )
        })?;

    match resp.status().as_u16() {
        200 => {
            let joined: Joined = resp
                .json()
                .await
                .context("the server paired this machine but its reply could not be read")?;
            if joined.token.is_empty() {
                bail!("The server at {server} paired this machine but sent no token.");
            }
            Ok(joined)
        }
        404 => bail!(
            "That code is not valid. Codes work once and expire after five minutes. \
             Run `collab pair` again on a machine that is already in the team."
        ),
        401 | 403 | 405 => bail!(
            "The server at {server} does not support pairing codes. Update it, or set \
             COLLAB_TOKEN to the team token instead."
        ),
        other => bail!("The server at {server} could not pair this machine (HTTP {other})."),
    }
}

/// Ask the server for a pairing code. Needs a team token.
pub async fn request_code(server: &str, token: &str) -> Result<PairCode> {
    let server = normalize_server(server);
    let resp = http()
        .post(format!("{server}/pair/codes"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| anyhow!("Can't reach {server}. ({e})"))?;

    match resp.status().as_u16() {
        200 => resp
            .json()
            .await
            .context("the server made a pairing code but its reply could not be read"),
        401 => bail!("The server at {server} did not accept this machine's token."),
        403 => bail!(
            "Pairing codes add a machine to a team, so they need that team's token. \
             The token in use here is an admin token."
        ),
        404 | 405 => bail!("The server at {server} does not support pairing codes. Update it."),
        other => bail!("The server at {server} could not make a pairing code (HTTP {other})."),
    }
}

/// Save a join into the config file at `path`, keeping everything else in
/// it. Creates the file and its directory if needed.
pub fn write_joined_config(
    path: &Path,
    server: &str,
    joined: &Joined,
    instance: Option<&str>,
) -> Result<()> {
    let server = normalize_server(server);
    let existing = std::fs::read_to_string(path).unwrap_or_default();

    let mut settings: Vec<(&str, &str)> = Vec::new();
    settings.push(("host", server.as_str()));
    settings.push(("token", joined.token.as_str()));
    if !joined.team_name.is_empty() {
        settings.push(("team", joined.team_name.as_str()));
    }
    if let Some(name) = instance {
        settings.push(("instance", name));
    }
    let text = merge_config(&existing, &settings);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("could not write {}", path.display()))?;
    restrict_to_owner(path);
    Ok(())
}

/// The file holds a token: keep other users on the machine out of it.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) {}

/// Things that would make a fresh join look as if it had not worked: a
/// setting in the environment, or a config file nearer the working
/// directory, beats what was just saved. `env` looks a variable up.
pub fn override_warnings(
    env: impl Fn(&str) -> Option<String>,
    server: &str,
    local_config: Option<&Path>,
) -> Vec<String> {
    let server = normalize_server(server);
    let mut out = Vec::new();
    if let Some(v) = env("COLLAB_SERVER") {
        if normalize_server(&v) != server {
            out.push(format!(
                "COLLAB_SERVER is set to {v} in this shell and overrides the saved address. \
                 Unset it (and remove it from your shell profile) to use {server}."
            ));
        }
    }
    if env("COLLAB_TOKEN").is_some() {
        out.push(
            "COLLAB_TOKEN is set in this shell and overrides the saved token. Unset it \
             (and remove it from your shell profile) to use the one from this join."
                .to_string(),
        );
    }
    if let Some(path) = local_config {
        out.push(format!(
            "{} is nearer than the saved config and overrides it for commands run from \
             this directory.",
            path.display()
        ));
    }
    out
}

/// Where `collab join` saves the result.
pub fn global_config_path() -> Option<PathBuf> {
    crate::home::home_dir().map(|h| h.join(".collab.toml"))
}

/// `collab join <address> <code>`.
pub async fn run_join(
    address: &str,
    code: &str,
    instance: Option<&str>,
    local_config: Option<&Path>,
) -> Result<()> {
    let server = normalize_server(address);
    let joined = redeem(&server, code).await?;
    let path = global_config_path()
        .ok_or_else(|| anyhow!("Could not work out the home directory. Set COLLAB_HOME."))?;
    write_joined_config(&path, &server, &joined, instance)?;

    let team = if joined.team_name.is_empty() {
        "the team"
    } else {
        joined.team_name.as_str()
    };
    println!("✓ Joined {team} at {server}");
    println!("  Saved to {}", path.display());
    match instance {
        Some(name) => println!("  This machine posts as @{name}."),
        None => println!(
            "  Pick a name for each session: collab --instance <name> status   (or set COLLAB_INSTANCE)"
        ),
    }
    for warning in override_warnings(|k| std::env::var(k).ok(), &server, local_config) {
        eprintln!("  (warning) {warning}");
    }
    Ok(())
}

/// `collab pair`.
pub async fn run_pair(server: &str, token: Option<&str>) -> Result<()> {
    let token = token.ok_or_else(|| {
        anyhow!("This machine is not in a team yet. Join one first with `collab join <address> <code>`.")
    })?;
    let server = normalize_server(server);
    let pair = request_code(&server, token).await?;
    let minutes = (pair.expires_in / 60).max(1);

    println!("Pairing code:  {}", display_code(&pair.code));
    println!();
    println!("On the new machine run:");
    println!("  collab join {server} {}", pair.code);
    println!();
    println!(
        "The code works once and expires in {minutes} minute{}.",
        if minutes == 1 { "" } else { "s" }
    );
    if crate::join_text::is_loopback_server(&server) {
        println!(
            "Note: {server} only means this machine. Give the new machine this machine's \
             network address instead."
        );
    }
    Ok(())
}

/// Ask the server which team `token` belongs to, for `collab whoami`. Never
/// fails: anything short of a clear answer is `Unknown`, so the caller can
/// still print everything else it knows.
pub async fn team_of_token(server: &str, token: Option<&str>) -> crate::whoami_text::ServerTeam {
    use crate::whoami_text::ServerTeam;
    let Some(token) = token else {
        return ServerTeam::Unknown;
    };
    let server = normalize_server(server);
    let Ok(resp) = http()
        .get(format!("{server}/team"))
        .header("Authorization", format!("Bearer {token}"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    else {
        return ServerTeam::Unknown;
    };
    let ok = resp.status().is_success();
    // A server from before /team answers 404 with no JSON.
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return ServerTeam::Unknown;
    };
    match (ok, body["name"].as_str(), body["error"].as_str()) {
        (true, Some(name), _) => ServerTeam::Named(name.to_string()),
        (false, _, Some("no_team")) => ServerTeam::NoTeam,
        _ => ServerTeam::Unknown,
    }
}
