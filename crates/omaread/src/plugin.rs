//! The Omarchy bar widget: carried in the binary, written to the shell's
//! plugin folder, and enabled through `omarchy plugin`.
//!
//!   omaread plugin install   put it on the bar
//!   omaread plugin update    rewrite its files (the app does this on start)
//!   omaread plugin remove    take it off and delete it
//!   omaread plugin status    say where it is

use std::path::PathBuf;
use std::process::{Command, Stdio};

use gtk::glib;

pub const ID: &str = "sarrietav-dev.omaread";

/// The service's IPC target. The bar widget opens and closes on `ID`; asking
/// the service to re-read `omaread status` is a separate call.
const SERVICE_TARGET: &str = "sarrietav-dev.omaread-service";

const FILES: &[(&str, &str)] = &[
    ("manifest.json", include_str!("../../../plugin/manifest.json")),
    ("Model.js", include_str!("../../../plugin/Model.js")),
    ("ServiceHost.js", include_str!("../../../plugin/ServiceHost.js")),
    ("Service.qml", include_str!("../../../plugin/Service.qml")),
    ("Panel.qml", include_str!("../../../plugin/Panel.qml")),
    ("ReadingIcon.qml", include_str!("../../../plugin/ReadingIcon.qml")),
];

fn plugins_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/omarchy/plugins")
}

fn location() -> PathBuf {
    plugins_dir().join(ID)
}

/// Is this an Omarchy desktop with a shell that takes plugins?
pub fn available() -> bool {
    plugins_dir().is_dir() && which("omarchy-shell")
}

pub fn installed() -> bool {
    location().join("manifest.json").is_file()
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(program).is_file()))
}

fn quiet(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if status.status.success() {
        Ok(())
    } else {
        Err(format!("{program} {}: {}", args.join(" "), String::from_utf8_lossy(&status.stderr).trim()))
    }
}

/// Write the files; `true` if anything changed.
fn write_files() -> Result<bool, String> {
    let target = location();
    std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut changed = false;
    for (name, contents) in FILES {
        let path = target.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|current| current == *contents) {
            continue;
        }
        std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
        changed = true;
    }
    Ok(changed)
}

pub fn install() -> Result<(), String> {
    if !available() {
        return Err("this doesn't look like an Omarchy desktop (no omarchy-shell or plugin folder)".into());
    }
    write_files()?;
    quiet("omarchy-shell", &["-q", "shell", "rescanPlugins"])?;
    quiet("omarchy", &["plugin", "enable", ID])
}

/// Keep an installed widget in step with this build. Quiet and best effort.
pub fn refresh() {
    if installed() && matches!(write_files(), Ok(true)) {
        let _ = quiet("omarchy-shell", &["-q", "shell", "rescanPlugins"]);
    }
}

pub fn remove() -> Result<(), String> {
    if !installed() {
        return Ok(());
    }
    let _ = quiet("omarchy", &["plugin", "disable", ID]);
    std::fs::remove_dir_all(location()).map_err(|e| e.to_string())?;
    let _ = quiet("omarchy-shell", &["-q", "shell", "rescanPlugins"]);
    Ok(())
}

/// Tell the widget something changed (a book closed). Fire and forget.
pub fn notify_changed() {
    if !installed() {
        return;
    }
    let _ = Command::new("omarchy-shell")
        .args(["-q", SERVICE_TARGET, "refresh"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn command(sub: Option<&str>) -> glib::ExitCode {
    let result = match sub {
        Some("install") => install().map(|()| "The Omaread widget is on the bar.".to_string()),
        Some("update") => {
            write_files().map(|changed| if changed { "Updated the widget." } else { "The widget is up to date." }.to_string())
        }
        Some("remove") => remove().map(|()| "Removed the widget.".to_string()),
        Some("status") | None => Ok(if installed() {
            format!("Installed at {}", location().display())
        } else if available() {
            "Not installed. omaread plugin install puts it on the bar.".to_string()
        } else {
            "Not available: this isn't an Omarchy desktop.".to_string()
        }),
        Some(other) => Err(format!("unknown: {other} (install, update, remove, status)")),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            glib::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("omaread: {e}");
            glib::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_plugin_declares_itself_and_has_its_entry_points() {
        let manifest: serde_json::Value = serde_json::from_str(FILES[0].1).unwrap();
        assert_eq!(manifest["id"], ID);
        for entry in manifest["entryPoints"].as_object().unwrap().values() {
            let name = entry.as_str().unwrap();
            assert!(FILES.iter().any(|(f, _)| *f == name), "{name} is not embedded");
        }
        // Every QML/JS file the plugin imports is embedded.
        for (_, contents) in FILES {
            for line in contents.lines().filter(|l| l.trim_start().starts_with("import \"")) {
                let file = line.split('"').nth(1).unwrap();
                assert!(FILES.iter().any(|(f, _)| *f == file), "{file} is imported but not embedded");
            }
        }
        // The app pings the service by name; keep the two in step.
        let service = FILES.iter().find(|(f, _)| *f == "Service.qml").unwrap().1;
        assert!(service.contains(&format!("target: \"{SERVICE_TARGET}\"")), "Service.qml no longer answers {SERVICE_TARGET}");
    }
}
