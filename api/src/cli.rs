//! Server-side commands, run inside the API container instead of serving:
//!
//! ```text
//! docker exec deploy-api-1 ember-trove-api backup create [--comment TEXT]
//! ```
//!
//! `backup create` makes the same backup as the admin's button
//! (`backup::create_backup`), recorded as made by
//! `common::backup::AUTOMATIC_ACTOR`, then deletes the automatic backups past
//! the newest `AUTOMATIC_KEPT`. The release workflow runs it after each
//! deploy (2026-11-02).
//!
//! There is no sign-in: running it takes a shell on the server, which can
//! already read the database and storage credentials. Nothing new listens on
//! the network, and no new credential exists.

use common::backup::{AUTOMATIC_ACTOR, AUTOMATIC_KEPT};

use crate::{backup, state::AppState};

const USAGE: &str = "usage: ember-trove-api [backup create [--comment TEXT]]";

/// What to run instead of the server.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    BackupCreate { comment: Option<String> },
}

/// The command in the arguments (without the program name). `Ok(None)`
/// means no arguments: serve as usual. Anything unrecognised is an error,
/// so a typo never starts a second server.
pub fn parse(args: &[String]) -> Result<Option<Command>, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => Ok(None),
        ["backup", "create"] => Ok(Some(Command::BackupCreate { comment: None })),
        ["backup", "create", "--comment", c] if !c.trim().is_empty() => {
            Ok(Some(Command::BackupCreate {
                comment: Some(c.trim().to_string()),
            }))
        }
        _ => Err(USAGE.to_string()),
    }
}

/// Run `command` and report on stdout. Counts and ids only: no content.
pub async fn run(command: Command, state: &AppState) -> anyhow::Result<()> {
    match command {
        Command::BackupCreate { comment } => {
            let job = backup::create_backup(state, AUTOMATIC_ACTOR, comment.as_deref())
                .await
                .map_err(|e| anyhow::anyhow!("backup failed: {e}"))?;
            println!(
                "backup {} created: {} bytes, {} nodes, {} tasks, {} notes, {} attachments",
                job.id,
                job.size_bytes,
                job.node_count,
                job.task_count,
                job.note_count,
                job.attachment_count
            );
            let pruned = backup::prune_automatic(
                state.backup.as_ref(),
                state.object_store.as_ref(),
                state.object_deletions.as_ref(),
                AUTOMATIC_KEPT,
            )
            .await
            .map_err(|e| anyhow::anyhow!("pruning old automatic backups failed: {e}"))?;
            println!("deleted {pruned} automatic backup(s) past the newest {AUTOMATIC_KEPT}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_serves() {
        assert_eq!(parse(&[]), Ok(None));
    }

    #[test]
    fn backup_create_takes_an_optional_comment() {
        assert_eq!(
            parse(&args(&["backup", "create"])),
            Ok(Some(Command::BackupCreate { comment: None }))
        );
        assert_eq!(
            parse(&args(&[
                "backup",
                "create",
                "--comment",
                "  After v3.11.0 "
            ])),
            Ok(Some(Command::BackupCreate {
                comment: Some("After v3.11.0".into())
            }))
        );
    }

    #[test]
    fn anything_else_is_refused_with_the_usage() {
        for bad in [
            &["backup"][..],
            &["backup", "delete"],
            &["backup", "create", "--comment"],
            &["backup", "create", "--comment", "   "],
            &["backup", "create", "extra"],
            &["serve"],
            &["--help"],
        ] {
            assert_eq!(parse(&args(bad)), Err(USAGE.to_string()), "{bad:?}");
        }
    }
}
