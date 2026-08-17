# Changelog

本项目的所有重要变更都会记录在此文件中。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循[语义化版本](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.1.1] - 2026-08-17

### Fixed

- 修复原文区和翻译区无法使用鼠标滚轮及覆盖式滚动条的问题
- 修复滚动条隐藏后的残留像素和滚动时的文字闪烁
- 修复长文本触发界面消息重入时可能卡死的问题
- 缓存翻译结果默认从顶部显示，不再自动滚动到底部

## [0.1.0] - 2026-08-17

### Added

- Windows 全局快捷键划词翻译与可编辑原文框
- OpenAI Responses、Chat Completions 和 Completions API 支持
- 多服务商托盘切换与流式结果浮窗
- 本地语言识别、SQLite LRU 缓存及开机自启
- 可配置窗口定位、置顶、尺寸和上下分隔比例

[Unreleased]: https://github.com/4fuu/fy/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/4fuu/fy/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/4fuu/fy/releases/tag/v0.1.0
