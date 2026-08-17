use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG: &str = r#"# fy 配置文件
# 修改后在托盘菜单选择“重新加载配置”。服务商也可以直接从托盘菜单切换。

# 当前服务商，必须与下面某个 [[providers]] 的 name 完全一致。
active_provider = "OpenAI"

# 语言使用 BCP 47/ISO 639 代码或英文名称。识别为 user_language 时翻译到
# second_language，否则翻译到 user_language。
user_language = "zh-CN"
second_language = "en"

# api_format 必须显式填写：responses、chat_completions 或 completions。
# stream 默认开启；设为 false 时会等待完整响应后一次性显示。
# extra_params 会作为 JSON 顶层字段追加到请求，适合 reasoning_effort、top_p 等服务商参数。
[[providers]]
name = "OpenAI"
api_key = ""
base_url = "https://api.openai.com/v1"
model = "gpt-4.1-mini"
api_format = "responses"
stream = true
system_prompt = "You are a translation engine. Translate into {target_language}. Preserve meaning, tone, formatting, and proper nouns. Return only the translation."
user_prompt = "{text}"
temperature = 0.2
max_output_tokens = 2048
request_timeout_seconds = 60
extra_params = {}

[[providers]]
name = "DeepSeek"
api_key = ""
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
api_format = "chat_completions"
stream = true
system_prompt = "You are a translation engine. Translate into {target_language}. Preserve meaning, tone, formatting, and proper nouns. Return only the translation."
user_prompt = "{text}"
temperature = 0.2
max_output_tokens = 2048
request_timeout_seconds = 60
extra_params = {}

[app]
# 全局快捷键支持 Ctrl、Alt、Shift、Win 加一个字母或数字。
hotkey = "Alt+X"
cache_max_mb = 10
autostart = false

# auto 会在显示浮窗时跟随鼠标并限制在当前显示器内；fixed 使用 window_x/window_y。
window_position = "auto"
window_width = 480
window_height = 520
input_ratio = 0.4
# window_x = 100
# window_y = 100
# 普通显示浮窗时的初始置顶状态；快捷键未复制到文本时仍会自动切换为置顶。
always_on_top = false
"#;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub active_provider: String,
    pub user_language: String,
    pub second_language: String,
    pub providers: Vec<ProviderConfig>,
    pub app: AppConfig,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    Responses,
    ChatCompletions,
    Completions,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowPosition {
    Auto,
    Fixed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    pub model: String,
    pub api_format: ApiFormat,
    #[serde(default = "default_stream")]
    pub stream: bool,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default = "default_user_prompt")]
    pub user_prompt: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub max_output_tokens: u32,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_seconds: u64,
    #[serde(default)]
    pub extra_params: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_cache_mb")]
    pub cache_max_mb: u64,
    #[serde(default = "default_window_width")]
    pub window_width: i32,
    #[serde(default = "default_window_height")]
    pub window_height: i32,
    #[serde(default = "default_input_ratio")]
    pub input_ratio: f32,
    #[serde(default = "default_window_position")]
    pub window_position: WindowPosition,
    #[serde(default)]
    pub window_x: Option<i32>,
    #[serde(default)]
    pub window_y: Option<i32>,
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default)]
    pub autostart: bool,
}

impl Config {
    pub fn load_or_create() -> Result<(Self, Paths)> {
        let paths = Paths::discover()?;
        Self::load_or_create_at(paths)
    }

    fn load_or_create_at(paths: Paths) -> Result<(Self, Paths)> {
        fs::create_dir_all(&paths.root)
            .with_context(|| format!("无法创建 {}", paths.root.display()))?;
        if !paths.config.exists() {
            fs::write(&paths.config, DEFAULT_CONFIG)
                .with_context(|| format!("无法创建 {}", paths.config.display()))?;
        }
        let text = fs::read_to_string(&paths.config)
            .with_context(|| format!("无法读取 {}", paths.config.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("配置格式错误: {}", paths.config.display()))?;
        config.validate()?;
        Ok((config, paths))
    }

    pub fn active_provider(&self) -> &ProviderConfig {
        self.providers
            .iter()
            .find(|provider| provider.name == self.active_provider)
            .expect("配置已验证，当前服务商必定存在")
    }

    pub fn select_provider(&mut self, paths: &Paths, name: &str) -> Result<()> {
        if !self.providers.iter().any(|provider| provider.name == name) {
            bail!("服务商不存在: {name}");
        }
        persist_active_provider(paths, name)?;
        self.active_provider = name.to_owned();
        Ok(())
    }

    pub fn set_input_ratio(&mut self, paths: &Paths, ratio: f32) -> Result<()> {
        if !(0.2..=0.8).contains(&ratio) {
            bail!("input_ratio 必须在 0.2 到 0.8 之间");
        }
        persist_input_ratio(paths, ratio)?;
        self.app.input_ratio = ratio;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.user_language.trim().is_empty() || self.second_language.trim().is_empty() {
            bail!("user_language 和 second_language 不能为空");
        }
        if self
            .user_language
            .eq_ignore_ascii_case(&self.second_language)
        {
            bail!("user_language 和 second_language 不能相同");
        }
        if self.providers.is_empty() {
            bail!("至少需要配置一个 [[providers]]");
        }
        if self.providers.len() > 1000 {
            bail!("服务商数量不能超过 1000");
        }
        let mut names = HashSet::new();
        for provider in &self.providers {
            let name = provider.name.trim();
            if name.is_empty() {
                bail!("providers.name 不能为空");
            }
            if !names.insert(name.to_ascii_lowercase()) {
                bail!("服务商名称不能重复: {}", provider.name);
            }
            if provider.model.trim().is_empty() {
                bail!("服务商 {} 的 model 不能为空", provider.name);
            }
            if provider.base_url.trim().is_empty() {
                bail!("服务商 {} 的 base_url 不能为空", provider.name);
            }
            let prompts = format!("{}\n{}", provider.system_prompt, provider.user_prompt);
            if !prompts.contains("{target_language}") || !prompts.contains("{text}") {
                bail!(
                    "服务商 {} 的 system_prompt 与 user_prompt 必须包含 {{target_language}} 和 {{text}}",
                    provider.name
                );
            }
            if !(0.0..=2.0).contains(&provider.temperature) {
                bail!("服务商 {} 的 temperature 必须在 0 到 2 之间", provider.name);
            }
            if provider.max_output_tokens == 0 || provider.request_timeout_seconds == 0 {
                bail!("服务商 {} 的 token 上限和请求超时必须大于 0", provider.name);
            }
            validate_extra_params(provider)?;
        }
        if !self
            .providers
            .iter()
            .any(|provider| provider.name == self.active_provider)
        {
            bail!("active_provider 未匹配任何服务商: {}", self.active_provider);
        }
        if self.app.cache_max_mb == 0 {
            bail!("app.cache_max_mb 必须大于 0");
        }
        if self.app.window_width < 360 || self.app.window_height < 300 {
            bail!("浮窗尺寸至少为 360x300");
        }
        if !(0.2..=0.8).contains(&self.app.input_ratio) {
            bail!("app.input_ratio 必须在 0.2 到 0.8 之间");
        }
        if self.app.window_position == WindowPosition::Fixed
            && (self.app.window_x.is_none() || self.app.window_y.is_none())
        {
            bail!("window_position = \"fixed\" 时必须同时配置 window_x 和 window_y");
        }
        Ok(())
    }

    pub fn cache_limit_bytes(&self) -> u64 {
        self.app.cache_max_mb.saturating_mul(1024 * 1024)
    }
}

fn validate_extra_params(provider: &ProviderConfig) -> Result<()> {
    let mut reserved = vec!["model", "temperature", "stream"];
    match provider.api_format {
        ApiFormat::Responses => reserved.extend(["instructions", "input", "max_output_tokens"]),
        ApiFormat::ChatCompletions => {
            reserved.extend(["messages", "max_completion_tokens", "max_tokens"])
        }
        ApiFormat::Completions => reserved.extend(["prompt", "max_tokens"]),
    }
    if let Some(key) = provider
        .extra_params
        .keys()
        .find(|key| reserved.contains(&key.as_str()))
    {
        bail!(
            "服务商 {} 的 extra_params 不能覆盖保留参数 {key}",
            provider.name
        );
    }
    Ok(())
}

fn persist_active_provider(paths: &Paths, name: &str) -> Result<()> {
    let text = fs::read_to_string(&paths.config)
        .with_context(|| format!("无法读取 {}", paths.config.display()))?;
    let value = toml::Value::String(name.to_owned()).to_string();
    let mut replaced = false;
    let mut output = String::with_capacity(text.len());
    for line in text.lines() {
        if !replaced && line.trim_start().starts_with("active_provider =") {
            output.push_str("active_provider = ");
            output.push_str(&value);
            replaced = true;
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    if !replaced {
        output.insert_str(0, &format!("active_provider = {value}\n\n"));
    }
    fs::write(&paths.config, output)
        .with_context(|| format!("无法保存当前服务商到 {}", paths.config.display()))
}

fn persist_input_ratio(paths: &Paths, ratio: f32) -> Result<()> {
    let text = fs::read_to_string(&paths.config)
        .with_context(|| format!("无法读取 {}", paths.config.display()))?;
    let value = format!("{ratio:.3}");
    let has_value = text
        .lines()
        .any(|line| line.trim_start().starts_with("input_ratio ="));
    let mut output = String::with_capacity(text.len() + 24);
    for line in text.lines() {
        if line.trim_start().starts_with("input_ratio =") {
            output.push_str("input_ratio = ");
            output.push_str(&value);
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
            if !has_value && line.trim() == "[app]" {
                output.push_str("input_ratio = ");
                output.push_str(&value);
                output.push('\n');
            }
        }
    }
    fs::write(&paths.config, output)
        .with_context(|| format!("无法保存输入框比例到 {}", paths.config.display()))
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    fn discover() -> Result<Self> {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .context("无法确定用户主目录")?;
        let root = home.join(".config").join("fy");
        Ok(Self {
            config: root.join("config.toml"),
            cache: root.join("cache.sqlite3"),
            root,
        })
    }
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".into()
}
fn default_stream() -> bool {
    true
}
fn default_system_prompt() -> String {
    "Translate into {target_language}. Return only the translation.".into()
}
fn default_user_prompt() -> String {
    "{text}".into()
}
fn default_temperature() -> f32 {
    0.2
}
fn default_max_tokens() -> u32 {
    2048
}
fn default_request_timeout() -> u64 {
    60
}
fn default_hotkey() -> String {
    "Alt+X".into()
}
fn default_cache_mb() -> u64 {
    10
}
fn default_window_width() -> i32 {
    480
}
fn default_window_height() -> i32 {
    520
}
fn default_input_ratio() -> f32 {
    0.4
}
fn default_window_position() -> WindowPosition {
    WindowPosition::Auto
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        config.validate().unwrap();
        assert_eq!(config.providers.len(), 2);
        assert!(config.providers.iter().all(|provider| provider.stream));
        assert_eq!(config.active_provider().name, "OpenAI");
        assert_eq!(config.cache_limit_bytes(), 10 * 1024 * 1024);
        assert_eq!(config.app.window_width, 480);
        assert_eq!(config.app.window_height, 520);
        assert_eq!(config.app.input_ratio, 0.4);
        assert!(!config.app.always_on_top);
    }

    #[test]
    fn existing_configs_default_to_streaming() {
        let previous_template = DEFAULT_CONFIG.replace("stream = true\n", "");
        let config: Config = toml::from_str(&previous_template).unwrap();
        assert!(config.providers.iter().all(|provider| provider.stream));
    }

    #[test]
    fn rejects_missing_active_provider() {
        let mut config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        config.active_provider = "missing".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn provider_selection_is_persisted_without_removing_comments() {
        let root = std::env::temp_dir().join(format!("fy-config-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config: root.join("config.toml"),
            cache: root.join("cache.sqlite3"),
            root: root.clone(),
        };
        fs::write(&paths.config, DEFAULT_CONFIG).unwrap();
        let mut config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        config.select_provider(&paths, "DeepSeek").unwrap();

        let saved = fs::read_to_string(&paths.config).unwrap();
        let reloaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(reloaded.active_provider, "DeepSeek");
        assert!(saved.contains("# fy 配置文件"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn input_ratio_is_persisted_without_duplicate_keys() {
        let root = std::env::temp_dir().join(format!("fy-ratio-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config: root.join("config.toml"),
            cache: root.join("cache.sqlite3"),
            root: root.clone(),
        };
        fs::write(&paths.config, DEFAULT_CONFIG).unwrap();
        let mut config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        config.set_input_ratio(&paths, 0.65).unwrap();

        let saved = fs::read_to_string(&paths.config).unwrap();
        let reloaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(reloaded.app.input_ratio, 0.65);
        assert_eq!(saved.matches("input_ratio =").count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn first_run_creates_the_full_template() {
        let root = std::env::temp_dir().join(format!("fy-first-run-test-{}", std::process::id()));
        let paths = Paths {
            config: root.join("config.toml"),
            cache: root.join("cache.sqlite3"),
            root: root.clone(),
        };
        let (config, _) = Config::load_or_create_at(paths.clone()).unwrap();

        assert_eq!(config.providers.len(), 2);
        assert_eq!(fs::read_to_string(paths.config).unwrap(), DEFAULT_CONFIG);
        fs::remove_dir_all(root).unwrap();
    }
}
