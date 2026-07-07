use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::process::Command;

const CONFIG_PATH: &str = "config.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub auto_attach_devices: Vec<String>,
    #[serde(default = "default_wsl_distro")]
    pub wsl_distro: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_attach_devices: Vec::new(),
            wsl_distro: detect_default_wsl_distro(),
        }
    }
}

fn default_wsl_distro() -> String {
    detect_default_wsl_distro()
}

pub fn load_config() -> Config {
    if let Ok(mut file) = File::open(CONFIG_PATH) {
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            if let Ok(mut config) = serde_json::from_str::<Config>(&contents) {
                if config.wsl_distro.trim().is_empty() {
                    config.wsl_distro = detect_default_wsl_distro();
                }
                return config;
            }
        }
    }
    Config::default()
}

pub fn save_config(config: &Config) {
    if let Ok(mut file) = File::create(CONFIG_PATH) {
        if let Ok(json) = serde_json::to_string_pretty(config) {
            let _ = file.write_all(json.as_bytes());
        }
    }
}

fn decode_wsl_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let utf16: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();
        String::from_utf16_lossy(&utf16)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

pub fn detect_default_wsl_distro() -> String {
    Command::new("wsl")
        .args(["-l", "-q"])
        .output()
        .ok()
        .map(|output| decode_wsl_output(&output.stdout))
        .map(|text| {
            text.lines()
                .map(|line| {
                    line.trim()
                        .trim_start_matches('\u{FEFF}')
                        .trim_start_matches('*')
                        .trim()
                })
                .find(|line| !line.is_empty())
                .unwrap_or("Ubuntu-24.04")
                .to_string()
        })
        .unwrap_or_else(|| "Ubuntu-24.04".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_wsl_distro() {
        let config = Config::default();
        assert!(!config.wsl_distro.is_empty());
    }
}
