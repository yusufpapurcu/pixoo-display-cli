use serde_json::Value;

pub fn human(command: &str, result: &Value) -> String {
    let mut text = render(command, result);
    if let Some(scene) = result["scene"].as_str() {
        text = format!("Scene '{scene}'.\n{text}");
    }
    if let Some(steps) = result["completed_steps"].as_array() {
        for step in steps {
            if step["operation"] == "brightness" {
                text.push_str(&format!(
                    "\nSystem brightness requested {}, observed {}.",
                    step["requested"], step["observed"]
                ));
            }
        }
    }
    text.push_str(&cleanup(result));
    text
}
fn render(command: &str, result: &Value) -> String {
    if result["dry_run"] == true {
        let plan = &result["plan"];
        let mut text = if let Some(path) = result["output"].as_str() {
            format!("Wrote preview to {path}. No device connection.")
        } else {
            format!(
                "Dry run. {} on Pixoo-Max, 32x32. No device connection.",
                plan["operation"].as_str().unwrap_or("operation")
            )
        };
        if let Some(frames) = plan["media"]["frames"].as_u64() {
            text.push_str(&format!(
                "\n{frames} frame(s), {} native bytes, {} initial storage chunks.",
                plan["media"]["native_bytes"], plan["media"]["initial_storage_chunks"]
            ));
        }
        if let Some(tab) = plan["tab"].as_u64() {
            text.push_str(&format!("\nTarget tab {tab}."));
        }
        if plan["replaces_artwork"] == true {
            text.push_str(" Execution replaces artwork and requires --replace.");
        }
        if let Some(brightness) = result["brightness"].as_u64() {
            text.push_str(&format!(
                "\nSet system brightness to {brightness} before the display operation."
            ));
        }
        return text;
    }
    if command == "brightness" && result["requested"].is_number() {
        return format!(
            "System brightness {}. Device reported {}.",
            result["requested"], result["observed"]
        );
    }
    if let Some(tab) = result["tab"].as_u64() {
        return if result.get("bytes").is_some() {
            format!("Tab {tab} transfer completed. {} bytes, {} chunks, {} retransmissions.\nCompletion evidence: {}. Power-cycle retention is unverified.",result["bytes"],result["chunks"],result["retransmissions"],result["completion"].as_str().unwrap_or("unknown"))
        } else {
            format!("Selected tab {tab}. Device returned custom-mode status.")
        };
    }
    if let Some(frames) = result["media"]["frames"].as_u64() {
        return format!(
            "Displayed {frames} frame(s) at 32x32. Device answered the final status query."
        );
    }
    if let Some(code) = result["status"]["mode_code"].as_u64() {
        return format!("System brightness {}. Display mode {code}, documented as {}.\nPhysical mode meaning on this firmware is unverified.",result["status"]["system_brightness"],result["status"]["documented_mode"].as_str().unwrap_or("unknown"));
    }
    if result["device_configuration"] == "valid" {
        return format!(
            "Selected device configuration is valid for Pixoo-Max at 32x32. Bluetooth was not tested.\n{}",
            result["ownership"].as_str().unwrap_or("")
        );
    }
    for (key, prefix) in [
        ("scene_added", "Saved local scene"),
        ("removed", "Removed local scene"),
        ("device_added", "Added local device"),
        ("default_device", "Default device"),
    ] {
        if command != "device.list" {
            if let Some(name) = result[key].as_str() {
                return format!("{prefix} '{name}'. No device connection.");
            }
        }
    }
    if let Some(scenes) = result["scenes"].as_object() {
        if scenes.is_empty() {
            return "No local scenes. Add one with 'pixo scene add NAME --tab 1'.".into();
        }
        return scenes
            .iter()
            .map(|(name, scene)| {
                format!(
                    "{name}\t{}{}",
                    scene_source(scene),
                    scene["brightness"]
                        .as_u64()
                        .map(|v| format!("\tbrightness {v}"))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(devices) = result["devices"].as_object() {
        if devices.is_empty() {
            return "No configured devices. Use 'pixo device add' or 'pixo device import-go'."
                .into();
        }
        return devices
            .iter()
            .map(|(name, device)| {
                format!(
                    "{} {name}\t{}\t{}\tchannel {}",
                    if result["default_device"] == *name {
                        "*"
                    } else {
                        " "
                    },
                    device["profile"].as_str().unwrap_or("unknown"),
                    device["address"].as_str().unwrap_or("unknown"),
                    device["channel"]
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if let Some(name) = result["name"].as_str() {
        return format!("Local scene '{name}'. {}", scene_source(&result["scene"]));
    }
    serde_json::to_string_pretty(result).expect("JSON values serialize")
}
fn scene_source(scene: &Value) -> String {
    match scene["source"].as_str() {
        Some("tab") => format!("device tab {}", scene["tab"]),
        Some("file") => format!("file {}", scene["file"].as_str().unwrap_or("unknown")),
        Some("text") => format!("text {}", scene["text"]),
        _ => "unknown source".into(),
    }
}
fn cleanup(result: &Value) -> String {
    result["cleanup_warning"]
        .as_str()
        .map(|warning| format!("\nConnection cleanup warning: {warning}"))
        .unwrap_or_default()
}
