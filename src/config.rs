use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct FdfConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(default)]
    pub features: Option<Features>,
    #[serde(default)]
    pub android: Option<PlatformConfig>,
    #[serde(default)]
    pub ios: Option<PlatformConfig>,
    #[serde(default)]
    pub windows: Option<PlatformConfig>,
    #[serde(default)]
    pub wasm: Option<PlatformConfig>,
}

fn default_target() -> String {
    "desktop".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Features {
    #[serde(default = "default_true")]
    pub window_controls: bool,
    #[serde(default = "default_true")]
    pub ipc: bool,
    #[serde(default = "default_true")]
    pub ffi: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Features {
    fn default() -> Self {
        Features {
            window_controls: true,
            ipc: true,
            ffi: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformConfig {
    #[serde(default)]
    pub package_name: Option<String>,
    #[serde(default)]
    pub bundle_id: Option<String>,
    #[serde(default = "default_min_sdk")]
    pub min_sdk: u32,
    #[serde(default = "default_target_sdk")]
    pub target_sdk: u32,
    #[serde(default = "default_deployment_target")]
    pub deployment_target: String,
    #[serde(default = "default_build_dir")]
    pub build_dir: String,
    #[serde(default)]
    pub shared_user_id: Option<String>,
}

fn default_min_sdk() -> u32 { 21 }
fn default_target_sdk() -> u32 { 33 }
fn default_deployment_target() -> String { "15.0".to_string() }
fn default_build_dir() -> String {
    "build".to_string()
}

impl PlatformConfig {
    pub fn build_dir_for_target(target: &str) -> String {
        match target {
            "android" => "build-android".to_string(),
            "ios" => "build-ios".to_string(),
            "windows" => "build-windows".to_string(),
            "wasm" => "build-wasm".to_string(),
            _ => "build".to_string(),
        }
    }
}

impl FdfConfig {
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read fdf.json: {}", e))?;
        Self::from_str(&content)
    }

    pub fn from_str(content: &str) -> Result<Self, String> {
        serde_json::from_str(content)
            .map_err(|e| format!("Failed to parse fdf.json: {}", e))
    }

    pub fn features_or_default(&self) -> Features {
        self.features.clone().unwrap_or_default()
    }
}
