//! The "team" lines of `collab whoami`. Text only, so it is easy to test.
//!
//! A team can be known from two places: a `team.yml` marker in this repo
//! (the machine that manages the workers) and the server, which knows which
//! team the token belongs to. A machine that joined with `collab join` has
//! only the second.

/// What the server said when asked which team the token is for.
#[derive(Debug, PartialEq, Eq)]
pub enum ServerTeam {
    /// The token belongs to this team.
    Named(String),
    /// The token is valid but is not a team token (an admin token).
    NoTeam,
    /// No answer: no token, server unreachable, or a server too old to say.
    Unknown,
}

/// `marker` is `(team, team.yml path)` from this repo, when there is one.
pub fn team_lines(marker: Option<(&str, &str)>, server: &ServerTeam) -> Vec<String> {
    let mut lines = Vec::new();
    match (marker, server) {
        (Some((team, source)), _) => {
            lines.push(format!("  team:       {team}"));
            lines.push(format!("  team.yml:   {source}"));
            if let ServerTeam::Named(theirs) = server
                && theirs != team
            {
                lines.push(format!(
                    "  warning:    the token is for team '{theirs}', not '{team}'"
                ));
            }
        }
        (None, ServerTeam::Named(team)) => {
            lines.push(format!(
                "  team:       {team} (joined; no team.yml in this repo)"
            ));
        }
        (None, ServerTeam::NoTeam) => {
            lines.push("  team:       <none> (this token is not a team token)".to_string());
        }
        (None, ServerTeam::Unknown) => {
            lines.push(
                "  team:       <unknown> (no team.yml in this repo, and the server did not say)"
                    .to_string(),
            );
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joined_machine_shows_the_team_the_server_names() {
        let lines = team_lines(None, &ServerTeam::Named("crabs".into()));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("crabs"), "{lines:?}");
        assert!(!lines[0].contains("legacy"), "{lines:?}");
    }

    #[test]
    fn a_managed_repo_shows_its_marker() {
        let lines = team_lines(
            Some(("crabs", "/p/team.yml")),
            &ServerTeam::Named("crabs".into()),
        );
        assert_eq!(
            lines,
            vec!["  team:       crabs", "  team.yml:   /p/team.yml"]
        );
    }

    #[test]
    fn a_token_for_a_different_team_is_called_out() {
        let lines = team_lines(
            Some(("crabs", "/p/team.yml")),
            &ServerTeam::Named("other".into()),
        );
        assert_eq!(lines.len(), 3);
        assert!(
            lines[2].contains("'other'") && lines[2].contains("'crabs'"),
            "{lines:?}"
        );
    }

    #[test]
    fn the_marker_still_shows_when_the_server_is_silent() {
        for server in [ServerTeam::Unknown, ServerTeam::NoTeam] {
            let lines = team_lines(Some(("crabs", "/p/team.yml")), &server);
            assert_eq!(lines.len(), 2, "{server:?}");
        }
    }

    #[test]
    fn nothing_known_says_so_without_calling_it_legacy() {
        let unknown = team_lines(None, &ServerTeam::Unknown);
        assert!(unknown[0].contains("<unknown>"), "{unknown:?}");
        let admin = team_lines(None, &ServerTeam::NoTeam);
        assert!(admin[0].contains("not a team token"), "{admin:?}");
    }
}
