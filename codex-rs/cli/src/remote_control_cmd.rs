use clap::Args;
use codex_app_server_daemon::LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE;
use codex_arg0::Arg0DispatchPaths;
use codex_utils_cli::CliConfigOverrides;

#[derive(Debug, Args)]
pub(crate) struct RemoteControlCommand {
    /// Emit machine-readable JSON.
    #[arg(long = "json", global = true)]
    json: bool,

    #[command(subcommand)]
    subcommand: Option<RemoteControlSubcommand>,
}

impl RemoteControlCommand {
    pub(crate) fn subcommand_name(&self) -> &'static str {
        match self.subcommand {
            None => "remote-control",
            Some(RemoteControlSubcommand::Start) => "remote-control start",
            Some(RemoteControlSubcommand::Stop) => "remote-control stop",
        }
    }
}

#[derive(Debug, Clone, Copy, clap::Subcommand)]
enum RemoteControlSubcommand {
    /// Legacy hosted remote control is disabled in Astral.
    Start,

    /// Legacy hosted remote control is disabled in Astral.
    Stop,
}

pub(crate) async fn run(
    command: RemoteControlCommand,
    arg0_paths: Arg0DispatchPaths,
    root_config_overrides: CliConfigOverrides,
) -> anyhow::Result<()> {
    let RemoteControlCommand {
        json: _,
        subcommand: _,
    } = command;
    let _ = (arg0_paths, root_config_overrides);
    anyhow::bail!(LEGACY_REMOTE_CONTROL_DISABLED_MESSAGE)
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use pretty_assertions::assert_eq;

    use crate::MultitoolCli;
    use crate::Subcommand;

    #[test]
    fn remote_control_subcommand_names_match_cli_shape() {
        let cli = MultitoolCli::try_parse_from(["astral", "remote-control"])
            .expect("remote-control should parse");
        let Some(Subcommand::RemoteControl(command)) = cli.subcommand else {
            panic!("expected remote-control subcommand");
        };
        assert_eq!(command.subcommand_name(), "remote-control");

        let cli = MultitoolCli::try_parse_from(["astral", "remote-control", "start"])
            .expect("remote-control start should parse");
        let Some(Subcommand::RemoteControl(command)) = cli.subcommand else {
            panic!("expected remote-control start subcommand");
        };
        assert_eq!(command.subcommand_name(), "remote-control start");

        let cli = MultitoolCli::try_parse_from(["astral", "remote-control", "stop"])
            .expect("remote-control stop should parse");
        let Some(Subcommand::RemoteControl(command)) = cli.subcommand else {
            panic!("expected remote-control stop subcommand");
        };
        assert_eq!(command.subcommand_name(), "remote-control stop");
    }
}
