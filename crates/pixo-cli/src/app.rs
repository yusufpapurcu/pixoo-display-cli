use crate::{
    cli::{Cli, Command, DeviceCommand, ModeCommand, SceneCommand},
    config::{percent, validate_name, Device, GoConfiguration, Scene, SceneSource, Store},
    device::{self, Action, Prepared},
    error::{CliError, Result},
    media::{self, Asset},
};
use divoom_bluetooth::{
    commands::{display::LightMode, Percent, Rgb},
    session::CancellationToken,
};
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf};
pub struct Outcome {
    pub device: Option<String>,
    pub result: Value,
}
impl Outcome {
    fn local(result: Value) -> Self {
        Self {
            device: None,
            result,
        }
    }
}
pub fn run(cli: &Cli, cancel: &CancellationToken) -> Result<Outcome> {
    check_cancel(cancel)?;
    let store = Store::new(cli.config.clone())?;
    match &cli.command {
        Command::Device { command } => device_config(command, &store),
        Command::Scene { command } => scene_command(command, cli, &store, cancel),
        Command::Doctor { connect: false } => {
            let (name, device) = store.device(cli.device.as_deref())?;
            Ok(Outcome {
                device: Some(name),
                result: json!({"device_configuration":"valid","config":store.path,"profile":device.profile,"size":device.size,
                "transport":"classic","connection_tested":false,"ownership":"OS lock coordinates pixo only; phones and legacy tools can still compete",
                "platform_supported":cfg!(any(windows,target_os="linux"))}),
            })
        }
        Command::Doctor { connect: true } | Command::Status => perform(
            Prepared {
                action: Action::Status,
                brightness: None,
            },
            None,
            false,
            cli,
            &store,
            cancel,
        ),
        Command::Show { file, options } => perform(
            Prepared {
                action: Action::display(Asset::file(file, options.fit)?)?,
                brightness: percent(options.output.brightness)?,
            },
            options.output.output.as_ref(),
            options.output.dry_run,
            cli,
            &store,
            cancel,
        ),
        Command::Text {
            input,
            style,
            options,
        } => {
            let text = if let Some(text) = &input.text {
                text.clone()
            } else {
                let bytes = if let Some(file) = &input.file {
                    media::read_bounded(file, 4096)?
                } else {
                    read_stdin(cancel)?
                };
                String::from_utf8(bytes).map_err(|_| CliError::invalid("text must be UTF-8"))?
            };
            perform(
                Prepared {
                    action: Action::display(Asset::text(&text, style)?)?,
                    brightness: percent(options.brightness)?,
                },
                options.output.as_ref(),
                options.dry_run,
                cli,
                &store,
                cancel,
            )
        }
        Command::Save {
            file,
            tab,
            replace,
            options,
        } => {
            require_replace(
                *replace,
                options.output.dry_run || options.output.output.is_some(),
            )?;
            perform(
                Prepared {
                    action: Action::Save {
                        asset: Asset::file(file, options.fit)?,
                        tab: *tab,
                    },
                    brightness: percent(options.output.brightness)?,
                },
                options.output.output.as_ref(),
                options.output.dry_run,
                cli,
                &store,
                cancel,
            )
        }
        Command::Stored {
            tab,
            brightness,
            dry_run,
        } => perform(
            Prepared {
                action: Action::Stored(*tab),
                brightness: percent(*brightness)?,
            },
            None,
            *dry_run,
            cli,
            &store,
            cancel,
        ),
        Command::Brightness { value, dry_run } => perform(
            Prepared {
                action: Action::Brightness(percent(*value)?),
                brightness: None,
            },
            None,
            *dry_run,
            cli,
            &store,
            cancel,
        ),
        Command::Blank { options } => perform(
            Prepared {
                action: Action::display(Asset::blank()?)?,
                brightness: percent(options.brightness)?,
            },
            options.output.as_ref(),
            options.dry_run,
            cli,
            &store,
            cancel,
        ),
        Command::Mode { mode } => {
            let (action, dry_run) = match mode {
                None => (Action::Status, false),
                Some(ModeCommand::Custom { dry_run }) => {
                    (Action::Mode(LightMode::UserDefined), *dry_run)
                }
                Some(ModeCommand::Light {
                    color,
                    brightness,
                    effect,
                    dry_run,
                }) => {
                    let c = media::color(color)?;
                    (
                        Action::Mode(LightMode::Light {
                            color: Rgb::new(c[0], c[1], c[2]),
                            brightness: Percent::new(*brightness)?,
                            effect: *effect,
                            on: true,
                        }),
                        *dry_run,
                    )
                }
            };
            perform(
                Prepared {
                    action,
                    brightness: None,
                },
                None,
                dry_run,
                cli,
                &store,
                cancel,
            )
        }
    }
}

fn perform(
    prepared: Prepared,
    output: Option<&PathBuf>,
    dry_run: bool,
    cli: &Cli,
    store: &Store,
    cancel: &CancellationToken,
) -> Result<Outcome> {
    check_cancel(cancel)?;
    if let Some(output) = output {
        let asset = prepared
            .media()
            .ok_or_else(|| CliError::invalid("this operation has no raster preview"))?;
        asset.preview(output)?;
        check_cancel(cancel)?;
        let mut result = prepared.describe();
        result["output"] = json!(output);
        result["device_connected"] = json!(false);
        return Ok(Outcome::local(result));
    }
    if dry_run {
        let device = match cli.device.as_deref() {
            Some(name) => Some(store.device(Some(name))?.0),
            None => None,
        };
        return Ok(Outcome {
            device,
            result: prepared.describe(),
        });
    }
    let (name, device) = store.device(cli.device.as_deref())?;
    let timeout = cli.timeout.unwrap_or_else(|| prepared.default_timeout());
    let result = device::execute(&device, prepared, timeout, cancel).map_err(|mut e| {
        e.device = Some(name.clone());
        e
    })?;
    Ok(Outcome {
        device: Some(name),
        result,
    })
}
fn require_replace(replace: bool, offline: bool) -> Result<()> {
    if !replace && !offline {
        Err(CliError::invalid(
            "saving replaces existing artwork; add --replace, or inspect with --dry-run",
        ))
    } else {
        Ok(())
    }
}

fn device_config(command: &DeviceCommand, store: &Store) -> Result<Outcome> {
    let result = match command {
        DeviceCommand::Capabilities => device::capabilities(),
        DeviceCommand::List => {
            let config = store.load()?;
            json!({"default_device":config.default_device,"devices":config.devices})
        }
        DeviceCommand::Use { name } => store.change(|config| {
            if !config.devices.contains_key(name) {
                return Err(CliError::invalid(format!("unknown device '{name}'")));
            }
            config.default_device = Some(name.clone());
            Ok(json!({"default_device":name}))
        })?,
        DeviceCommand::Add {
            name,
            address,
            channel,
            profile,
            size,
            controller,
        } => {
            let mut device = Device {
                address: address.clone(),
                channel: *channel,
                profile: profile.clone(),
                size: *size,
                controller: controller.clone(),
            };
            device.validate()?;
            add_device(store, name, device)?
        }
        DeviceCommand::ImportGo {
            file,
            name,
            profile,
            size,
        } => {
            let legacy: GoConfiguration =
                serde_json::from_slice(&media::read_bounded(file, 1024 * 1024)?)
                    .map_err(|e| CliError::invalid(format!("invalid Go configuration: {e}")))?;
            if legacy.transport != "rfcomm" {
                return Err(CliError::unsupported(
                    "import requires a Go rfcomm configuration",
                ));
            }
            let mut device = Device {
                address: legacy.mac,
                channel: legacy.channel,
                profile: profile.clone(),
                size: *size,
                controller: Some(legacy.listen_addr),
            };
            device.validate()?;
            add_device(store, name, device)?
        }
    };
    Ok(Outcome::local(result))
}
fn add_device(store: &Store, name: &str, device: Device) -> Result<Value> {
    validate_name(name)?;
    store.change(|config| {
        if config.devices.contains_key(name) {
            return Err(CliError::invalid(format!("device '{name}' already exists")));
        }
        config.devices.insert(name.to_owned(), device);
        Ok(json!({"device_added":name,"config":store.path,"device_connected":false}))
    })
}

fn scene_command(
    command: &SceneCommand,
    cli: &Cli,
    store: &Store,
    cancel: &CancellationToken,
) -> Result<Outcome> {
    match command {
        SceneCommand::List => Ok(Outcome::local(json!({"scenes":store.load()?.scenes}))),
        SceneCommand::Show { name } => Ok(Outcome::local(
            json!({"name":name,"scene":store.scene(name)?}),
        )),
        SceneCommand::Remove { name } => Ok(Outcome::local(store.change(|config| {
            config
                .scenes
                .remove(name)
                .ok_or_else(|| CliError::invalid(format!("unknown scene '{name}'")))?;
            Ok(json!({"removed":name,"device_connected":false}))
        })?)),
        SceneCommand::Add {
            name,
            tab,
            file,
            text,
            fit,
            brightness,
            replace,
            style,
        } => {
            validate_name(name)?;
            percent(*brightness)?;
            let source = match (tab, file, text) {
                (Some(tab), None, None) => SceneSource::Tab { tab: *tab },
                (None, Some(file), None) => {
                    Asset::file(file, *fit)?;
                    SceneSource::File {
                        file: std::fs::canonicalize(file)?,
                        fit: *fit,
                    }
                }
                (None, None, Some(text)) => {
                    Asset::text(text, style)?;
                    SceneSource::Text {
                        text: text.clone(),
                        style: style.clone(),
                    }
                }
                _ => {
                    return Err(CliError::invalid(
                        "scene add requires exactly one of --tab, --file or --text",
                    ))
                }
            };
            let scene = Scene {
                source,
                brightness: *brightness,
            };
            check_cancel(cancel)?;
            Ok(Outcome::local(store.change(|config| {
                if config.scenes.contains_key(name) && !*replace {
                    return Err(CliError::invalid(
                        "scene exists; add --replace to edit the local recipe",
                    ));
                }
                config.scenes.insert(name.clone(), scene);
                Ok(json!({"scene_added":name,"device_connected":false}))
            })?))
        }
        SceneCommand::Play { name, options } => {
            let scene = store.scene(name)?;
            let brightness = percent(options.brightness.or(scene.brightness))?;
            let action = match scene.source {
                SceneSource::Tab { tab } => {
                    validate_tab(tab)?;
                    Action::Stored(tab)
                }
                SceneSource::File { file, fit } => Action::display(Asset::file(&file, fit)?)?,
                SceneSource::Text { text, style } => Action::display(Asset::text(&text, &style)?)?,
            };
            let mut outcome = perform(
                Prepared { action, brightness },
                options.output.as_ref(),
                options.dry_run,
                cli,
                store,
                cancel,
            )?;
            outcome.result["scene"] = json!(name);
            Ok(outcome)
        }
        SceneCommand::Upload {
            name,
            tab,
            replace,
            dry_run,
        } => {
            require_replace(*replace, *dry_run)?;
            let scene = store.scene(name)?;
            let asset = match scene.source {
                SceneSource::Tab { .. } => {
                    return Err(CliError::invalid(
                        "a tab-reference scene has no host artwork to upload",
                    ))
                }
                SceneSource::File { file, fit } => Asset::file(&file, fit)?,
                SceneSource::Text { text, style } => Asset::text(&text, &style)?,
            };
            let mut outcome = perform(
                Prepared {
                    action: Action::Save { asset, tab: *tab },
                    brightness: percent(scene.brightness)?,
                },
                None,
                *dry_run,
                cli,
                store,
                cancel,
            )?;
            outcome.result["scene"] = json!(name);
            Ok(outcome)
        }
    }
}
fn validate_tab(tab: u8) -> Result<()> {
    if (1..=3).contains(&tab) {
        Ok(())
    } else {
        Err(CliError::invalid("scene tab must be 1, 2 or 3"))
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(CliError::new(
            "cancelled",
            130,
            "cancelled during local preparation; no device operation started",
        ))
    } else {
        Ok(())
    }
}
fn read_stdin(cancel: &CancellationToken) -> Result<Vec<u8>> {
    check_cancel(cancel)?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("pixo-stdin".into())
        .spawn(move || {
            let result = (|| -> Result<Vec<u8>> {
                let mut bytes = Vec::new();
                std::io::stdin().take(4097).read_to_end(&mut bytes)?;
                if bytes.len() > 4096 {
                    return Err(CliError::invalid("text input exceeds 4096 bytes"));
                }
                Ok(bytes)
            })();
            let _ = sender.send(result);
        })?;
    receive_input(receiver, cancel)
}
fn receive_input(
    receiver: std::sync::mpsc::Receiver<Result<Vec<u8>>>,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    loop {
        check_cancel(cancel)?;
        match receiver.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CliError::new(
                    "local_io",
                    1,
                    "stdin reader ended without a result",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_interrupts_waiting_for_stdin_before_eof() {
        let (_sender, receiver) = std::sync::mpsc::sync_channel(1);
        let cancel = CancellationToken::default();
        let signal = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            signal.cancel();
        });
        let error = receive_input(receiver, &cancel).unwrap_err();
        thread.join().unwrap();
        assert_eq!(error.code, 130);
        assert!(!error.uncertain);
    }
}
