<p align="center">
  <img src="assets/fy.svg" width="128" height="128" alt="fy 图标">
</p>

<h1 align="center">fy</h1>

<p align="center">
  一个轻量、原生、完全由配置文件驱动的 Windows 划词翻译工具。
</p>

<p align="center">
  <a href="https://github.com/4fuu/fy/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/4fuu/fy/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2F11-0078D4?logo=windows">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-GPL--3.0--only-blue.svg"></a>
</p>

fy 启动后驻留在系统托盘，不提供传统主界面。选中文字并按下快捷键，即可通过 OpenAI 或兼容 API 获得流式翻译结果；所有服务商、模型、提示词和窗口行为均通过 TOML 配置。

## 功能特性

- 默认使用 `Alt+X` 获取当前选中文字并翻译
- 原生浮窗流式展示结果，支持置顶、关闭、缩放和拖动上下分隔条
- 未获取到选中文字时自动置顶，可在可编辑原文框中粘贴或输入
- 本地识别原文语言：其他语言翻译为用户语言，用户语言翻译为第二语言
- 支持多个服务商，并可从托盘右键菜单即时切换
- 显式支持 OpenAI `responses`、`chat_completions` 和传统 `completions` API 格式
- 每个服务商可配置 Base URL、模型、API Key、双提示词、超时及任意附加参数
- SQLite LRU 缓存，默认上限 10 MiB，超限自动淘汰旧记录
- 支持当前用户开机自启
- 托盘图标内嵌到可执行文件，运行时不依赖外部资源

## 系统要求

- Windows 10 或 Windows 11（x86-64）
- 一个 OpenAI 或 OpenAI 兼容服务的 API Key

## 安装

### GitHub Releases

从 [Releases](https://github.com/4fuu/fy/releases) 下载 `fy.exe`，放到固定目录后直接运行。

### Scoop

```powershell
scoop bucket add fy https://github.com/4fuu/fy
scoop install fy
```

### 从源码构建

安装 Rust MSVC 工具链后执行：

```powershell
cargo build --release --locked
.\target\release\fy.exe
```

## 快速开始

1. 首次运行 `fy.exe`，程序会进入系统托盘并创建配置模板。
2. 打开 `%USERPROFILE%\.config\fy\config.toml`，填写服务商的 `api_key`。
3. 右键托盘图标，选择“重新加载配置”。
4. 在任意程序中选中文字并按 `Alt+X`。

如果没有获取到选中文字，fy 会打开并自动置顶空白浮窗。可在原文框粘贴或输入内容，再按 `Ctrl+Enter` 翻译。

## 配置

运行时文件均位于：

```text
~/.config/fy/config.toml
~/.config/fy/cache.sqlite3
```

首次运行会自动生成带注释的完整模板。下面是一个最小示例：

```toml
active_provider = "OpenAI"
user_language = "zh-CN"
second_language = "en"

[[providers]]
name = "OpenAI"
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
model = "gpt-4.1-mini"
api_format = "responses"
stream = true
system_prompt = "You are a translation engine. Translate into {target_language}. Return only the translation."
user_prompt = "{text}"
temperature = 0.2
max_output_tokens = 2048
request_timeout_seconds = 60
extra_params = {}

[app]
hotkey = "Alt+X"
cache_max_mb = 10
autostart = false
window_position = "auto"
window_width = 480
window_height = 520
input_ratio = 0.4
always_on_top = false
```

### 语言规则

`user_language` 和 `second_language` 接受 BCP 47、ISO 639 代码或英文语言名称：

- 原文不是 `user_language` 时，翻译为 `user_language`
- 原文是 `user_language` 时，翻译为 `second_language`

### API 格式与提示词

每个服务商必须显式设置 `api_format`：

| 值 | API |
| --- | --- |
| `responses` | OpenAI Responses API |
| `chat_completions` | `/chat/completions` |
| `completions` | 传统 `/completions` |

`system_prompt` 与 `user_prompt` 配合使用，两段提示词合计必须包含以下占位符：

- `{text}`：待翻译原文
- `{target_language}`：根据语言识别结果选择的目标语言

默认 `stream = true`。兼容服务不支持 SSE 时，可改为 `false`。

### 任意附加参数

`extra_params` 会作为 JSON 顶层字段追加到 API 请求。TOML 布尔值使用小写 `true` / `false`，例如：

```toml
extra_params = { enable_thinking = true, top_p = 0.9, reasoning_effort = "low" }
```

附加参数不能覆盖程序生成的 `model`、`stream`、提示词和 token 上限等保留字段。

### 窗口与开机启动

- `window_position = "auto"`：首次显示时出现在鼠标附近，并限制在当前显示器内
- `window_position = "fixed"`：需同时设置 `window_x` 与 `window_y`
- `input_ratio`：原文区初始占比，可设置为 `0.2` 到 `0.8`
- `always_on_top`：普通弹窗的初始置顶状态；没有获取到文字时仍会自动置顶
- `autostart = true`：写入当前用户的 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`

修改后通过托盘菜单“重新加载配置”生效。切换服务商时，fy 会把新的 `active_provider` 保存回配置文件。

## 快捷操作

| 操作 | 行为 |
| --- | --- |
| `Alt+X` | 获取当前选中文字并翻译 |
| `Ctrl+Enter` | 翻译原文框中的内容 |
| `Esc` | 隐藏浮窗 |
| 双击托盘图标 | 显示浮窗 |
| 右键托盘图标 | 显示浮窗、切换服务商、重新加载配置、打开配置目录或退出 |

未置顶时，点击浮窗以外区域会自动隐藏浮窗。

## 隐私与限制

- 原文会发送给当前配置的 AI 服务商；请阅读该服务商的隐私政策。
- API Key 以明文保存在当前用户配置目录，请勿提交或共享真实配置文件。
- SQLite 缓存保存在本机，可能包含翻译原文和结果。
- 普通权限进程无法可靠读取“以管理员身份运行”的程序中的选中文字，这是 Windows UIPI 的限制。

## 开发

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

发布 `v*` 标签时，[Release 工作流](.github/workflows/release.yml)会构建 `fy.exe`、生成 SHA-256 校验文件并创建 GitHub Release。Scoop 清单位于 [`bucket/fy.json`](bucket/fy.json)。

## 许可证

本项目按 [GNU General Public License v3.0](LICENSE) 发布。选择文本功能使用 GPL-3.0-only 的 [`selection`](https://crates.io/crates/selection) 库，因此分发版本同样采用 GPL-3.0-only。
