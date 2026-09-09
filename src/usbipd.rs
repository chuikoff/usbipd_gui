use std::process::Command;
use std::str;

pub const KNOWN_STATES: &[&str] = &["Shared (forced)", "Not shared", "Attached", "Shared"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDevice {
    pub bus_id: String,
    pub vid_pid: String,
    pub device_name: String,
    pub state: String,
}

pub fn validate_bus_id(bus_id: &str) -> Result<(), String> {
    let parts: Vec<&str> = bus_id.split('-').collect();
    if parts.len() != 2
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return Err(format!("Некорректный bus id: {bus_id}"));
    }
    Ok(())
}

pub fn validate_wsl_distro(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Некорректное имя WSL-дистрибутива".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!("Некорректное имя WSL-дистрибутива: {name}"));
    }
    Ok(())
}

pub fn parse_usbipd_list(output: &str) -> Vec<UsbDevice> {
    output
        .lines()
        .skip(2)
        .take_while(|line| !line.is_empty() && !line.contains("Persisted:"))
        .filter_map(parse_usbipd_line)
        .collect()
}

pub fn parse_usbipd_line(line: &str) -> Option<UsbDevice> {
    let line = line.trim_end();
    if line.is_empty() || line.starts_with("GUID") {
        return None;
    }

    let state = KNOWN_STATES
        .iter()
        .find(|known| line.ends_with(**known))
        .map(|s| (*s).to_string())?;

    let prefix = line[..line.len() - state.len()].trim_end();
    let mut parts = prefix.split_whitespace();
    let bus_id = parts.next()?.to_string();
    let vid_pid = parts.next()?.to_string();
    let device_name = parts.collect::<Vec<_>>().join(" ");

    if device_name.is_empty() {
        return None;
    }

    Some(UsbDevice {
        bus_id,
        vid_pid,
        device_name,
        state,
    })
}

pub fn format_device_display(device: &UsbDevice, auto_attach: bool) -> String {
    if auto_attach {
        format!(
            "{}: {} [{}] [Auto-Attach]",
            device.bus_id, device.device_name, device.state
        )
    } else {
        format!(
            "{}: {} [{}]",
            device.bus_id, device.device_name, device.state
        )
    }
}

pub fn extract_bus_id(display_text: &str) -> Option<String> {
    display_text.split(": ").next().map(str::to_string)
}

pub fn extract_state_from_display(display_text: &str) -> Option<String> {
    let text = display_text.trim_end();
    let text = text.strip_suffix(" [Auto-Attach]").unwrap_or(text);
    let end = text.rfind(']')?;
    let start = text[..end].rfind('[')?;
    Some(text[start + 1..end].to_string())
}

pub fn fetch_usb_devices() -> Result<Vec<UsbDevice>, String> {
    let output = Command::new("usbipd")
        .arg("list")
        .output()
        .map_err(|e| format!("Ошибка выполнения usbipd list: {e}"))?;

    let output_str = str::from_utf8(&output.stdout)
        .map_err(|e| format!("Ошибка декодирования вывода usbipd: {e}"))?;

    Ok(parse_usbipd_list(output_str))
}

pub fn get_device_state(bus_id: &str) -> Result<Option<String>, String> {
    validate_bus_id(bus_id)?;
    Ok(fetch_usb_devices()?
        .into_iter()
        .find(|device| device.bus_id == bus_id)
        .map(|device| device.state))
}

pub fn run_usbipd_command(args: &[&str]) -> Result<(), String> {
    let output = Command::new("usbipd")
        .args(args)
        .output()
        .map_err(|e| format!("Не удалось запустить usbipd: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        Err(if message.is_empty() {
            format!("usbipd завершился с кодом {}", output.status)
        } else {
            message
        })
    }
}

/// Elevate via PowerShell Start-Process. Arguments must already be validated
/// (bus id / distro) so they cannot break out of the ArgumentList array.
fn run_elevated_usbipd(args: &[&str]) -> Result<(), String> {
    let arg_list = args
        .iter()
        .map(|arg| format!("'{}'", arg.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    let ps_command = format!(
        "Start-Process -FilePath 'usbipd' -ArgumentList @({arg_list}) -Verb RunAs -Wait -WindowStyle Hidden"
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
        .output()
        .map_err(|e| format!("Не удалось запустить elevated-команду: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "Не удалось выполнить команду с правами администратора: {}",
            stderr.trim()
        ))
    }
}

pub fn run_usbipd_bind(bus_id: &str) -> Result<(), String> {
    validate_bus_id(bus_id)?;
    run_elevated_usbipd(&["bind", "--busid", bus_id, "--force"])
}

pub fn run_usbipd_unbind(bus_id: &str) -> Result<(), String> {
    validate_bus_id(bus_id)?;
    run_elevated_usbipd(&["unbind", "--busid", bus_id])
}

pub fn run_usbipd_attach(bus_id: &str, wsl_distro: &str) -> Result<(), String> {
    validate_bus_id(bus_id)?;
    validate_wsl_distro(wsl_distro)?;
    run_usbipd_command(&["attach", "--wsl", wsl_distro, "--busid", bus_id])
}

pub fn run_usbipd_detach(bus_id: &str) -> Result<(), String> {
    validate_bus_id(bus_id)?;
    run_usbipd_command(&["detach", "--busid", bus_id])
}

pub fn spawn_auto_attach(bus_id: &str, wsl_distro: &str) -> Result<std::process::Child, String> {
    validate_bus_id(bus_id)?;
    validate_wsl_distro(wsl_distro)?;
    Command::new("usbipd")
        .args([
            "attach",
            "--wsl",
            wsl_distro,
            "--busid",
            bus_id,
            "--auto-attach",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Не удалось запустить Auto-Attach: {e}"))
}

pub fn is_bindable_state(state: &str) -> bool {
    state == "Not shared"
}

pub fn is_unbindable_state(state: &str) -> bool {
    matches!(state, "Shared" | "Attached" | "Shared (forced)")
}

pub fn is_auto_attachable_state(state: &str) -> bool {
    state == "Shared" || state == "Shared (forced)"
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OUTPUT: &str = r"Connected:
BUSID  VID:PID    DEVICE                                                        STATE
2-7    058f:9540  Alcorlink USB Smart Card Reader                               Not shared
2-9    04a9:26b4  Canon MF4010 Series                                           Not shared
2-10   2912:0008  ATOL USB (COM4)                                               Shared
2-11   1a2c:2124  USB-устройство ввода                                          Attached
2-12   046d:c52f  Device Not Shared Name                                        Shared (forced)

Persisted:
GUID                                  DEVICE
";

    #[test]
    fn parses_usbipd_list() {
        let devices = parse_usbipd_list(SAMPLE_OUTPUT);
        assert_eq!(devices.len(), 5);
        assert_eq!(devices[0].bus_id, "2-7");
        assert_eq!(devices[0].state, "Not shared");
        assert_eq!(devices[2].state, "Shared");
        assert_eq!(devices[3].state, "Attached");
        assert_eq!(devices[4].state, "Shared (forced)");
        assert_eq!(devices[4].device_name, "Device Not Shared Name");
    }

    #[test]
    fn extracts_display_fields() {
        let display = "2-7: Alcorlink USB Smart Card Reader [Not shared] [Auto-Attach]";
        assert_eq!(extract_bus_id(display), Some("2-7".to_string()));
        assert_eq!(
            extract_state_from_display(display),
            Some("Not shared".to_string())
        );
    }

    #[test]
    fn formats_device_display() {
        let device = UsbDevice {
            bus_id: "2-7".to_string(),
            vid_pid: "058f:9540".to_string(),
            device_name: "Reader".to_string(),
            state: "Not shared".to_string(),
        };
        assert_eq!(
            format_device_display(&device, true),
            "2-7: Reader [Not shared] [Auto-Attach]"
        );
    }

    #[test]
    fn validates_bus_id() {
        assert!(validate_bus_id("2-7").is_ok());
        assert!(validate_bus_id("12-34").is_ok());
        assert!(validate_bus_id("").is_err());
        assert!(validate_bus_id("2").is_err());
        assert!(validate_bus_id("2-7;calc").is_err());
        assert!(validate_bus_id("2-7 & whoami").is_err());
        assert!(validate_bus_id("../x").is_err());
    }

    #[test]
    fn validates_wsl_distro() {
        assert!(validate_wsl_distro("Ubuntu-24.04").is_ok());
        assert!(validate_wsl_distro("Debian").is_ok());
        assert!(validate_wsl_distro("").is_err());
        assert!(validate_wsl_distro("Ubuntu & calc").is_err());
        assert!(validate_wsl_distro("Ubuntu';calc").is_err());
    }
}
