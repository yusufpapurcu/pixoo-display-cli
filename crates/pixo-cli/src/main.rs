mod app;
mod cli;
mod config;
mod device;
mod error;
mod media;
mod output;

use clap::Parser;
use cli::{Cli, Command, DeviceCommand, SceneCommand};

use divoom_bluetooth::session::CancellationToken;
use error::CliError;

use serde_json::json;
use std::{io::Write, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let wants_json = args
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--json");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            return emit_error(wants_json, "parse", None, CliError::invalid(e.to_string()));
        }
    };
    let command = command_name(&cli.command);
    let cancel = CancellationToken::default();
    let signal = cancel.clone();
    if let Err(e) = ctrlc::set_handler(move || signal.cancel()) {
        return emit_error(
            cli.json,
            command,
            cli.device.as_deref(),
            CliError::new("signal_setup", 1, e.to_string()),
        );
    }
    if cli.verbose {
        eprintln!("pixo {command}; one-shot Classic owner; save operations are never automatically replayed");
    }
    match app::run(&cli, &cancel) {
        Ok(outcome) => {
            let rendered = if cli.json {
                serde_json::to_string_pretty(&json!({"schema_version":1,"command":command,"device":outcome.device,"ok":true,"result":outcome.result})).expect("JSON values serialize")
            } else {
                output::human(command, &outcome.result)
            };
            match writeln!(std::io::stdout().lock(), "{rendered}") {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::from(1),
            }
        }
        Err(e) => emit_error(cli.json, command, cli.device.as_deref(), e),
    }
}
fn emit_error(json_output: bool, command: &str, device: Option<&str>, e: CliError) -> ExitCode {
    if json_output {
        let _ = writeln!(
            std::io::stdout().lock(),
            "{}",
            json!({"schema_version":1,"command":command,"device":e.device.as_deref().or(device),"ok":false,"error":e.value()})
        );
    } else {
        eprintln!("{}: {}", e.category, e.message);
        if e.uncertain {
            eprintln!("Device state may have changed. Reconnect before further work; do not automatically repeat a save.");
        }
        if !e.completed.is_empty() {
            eprintln!("Completed steps: {}", json!(e.completed));
        }
    }
    ExitCode::from(e.code)
}
fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Status => "status",
        Command::Show { .. } => "show",
        Command::Text { .. } => "text",
        Command::Save { .. } => "save",
        Command::Stored { .. } => "stored",
        Command::Brightness { .. } => "brightness",
        Command::Blank { .. } => "blank",
        Command::Mode { .. } => "mode",
        Command::Scene { command } => match command {
            SceneCommand::Add { .. } => "scene.add",
            SceneCommand::List => "scene.list",
            SceneCommand::Show { .. } => "scene.show",
            SceneCommand::Remove { .. } => "scene.remove",
            SceneCommand::Play { .. } => "scene.play",
            SceneCommand::Upload { .. } => "scene.upload",
        },
        Command::Device { command } => match command {
            DeviceCommand::Add { .. } => "device.add",
            DeviceCommand::ImportGo { .. } => "device.import-go",
            DeviceCommand::List => "device.list",
            DeviceCommand::Use { .. } => "device.use",
            DeviceCommand::Capabilities => "device.capabilities",
        },
        Command::Doctor { .. } => "doctor",
    }
}
