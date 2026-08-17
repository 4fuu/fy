use std::{collections::BTreeMap, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use async_openai::{Client, config::OpenAIConfig as ClientConfig};
use futures::StreamExt;
use serde_json::{Map, Value, json};

use crate::{
    cache::Cache,
    config::{ApiFormat, Config, ProviderConfig},
    language,
};

pub fn translate(
    config: &Config,
    cache_path: &Path,
    source: &str,
    mut on_delta: impl FnMut(&str, bool),
) -> Result<String> {
    let source = source.trim();
    if source.is_empty() {
        bail!("请输入需要翻译的文本");
    }
    let provider = config.active_provider();
    if provider.api_key.trim().is_empty() {
        bail!(
            "请先在 config.toml 中配置服务商 {} 的 api_key",
            provider.name
        );
    }

    let target_language =
        language::target_language(source, &config.user_language, &config.second_language);
    let system_prompt = render_prompt(&provider.system_prompt, source, target_language);
    let user_prompt = render_prompt(&provider.user_prompt, source, target_language);
    let extra_params = serde_json::to_string(&provider.extra_params)?;
    let temperature = provider.temperature.to_string();
    let max_output_tokens = provider.max_output_tokens.to_string();
    let key = Cache::key(&[
        source,
        target_language,
        &config.user_language,
        &config.second_language,
        &provider.name,
        &provider.model,
        api_format_name(provider.api_format),
        &system_prompt,
        &user_prompt,
        provider.base_url.trim_end_matches('/'),
        &temperature,
        &max_output_tokens,
        &extra_params,
    ]);
    let mut cache = Cache::open(cache_path, config.cache_limit_bytes())?;
    if let Some(value) = cache.get(&key)? {
        on_delta(&value, true);
        return Ok(value);
    }

    let body = build_request_body(provider, &system_prompt, &user_prompt)?;
    let client_config = ClientConfig::new()
        .with_api_key(provider.api_key.trim())
        .with_api_base(provider.base_url.trim_end_matches('/'));
    let client = Client::with_config(client_config);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let translated = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(provider.request_timeout_seconds),
            async {
                if provider.stream {
                    let mut stream = match provider.api_format {
                        ApiFormat::Responses => client.responses().create_stream_byot(body).await,
                        ApiFormat::ChatCompletions => client.chat().create_stream_byot(body).await,
                        ApiFormat::Completions => {
                            client.completions().create_stream_byot(body).await
                        }
                    }
                    .context("无法建立 AI 流式响应")?;
                    let mut translated = String::new();
                    while let Some(event) = stream.next().await {
                        let event: Value = event.context("读取 AI 流式响应失败")?;
                        if let Some(error) = extract_stream_error(&event) {
                            bail!("AI 流式响应失败：{error}");
                        }
                        if let Some(delta) = extract_stream_delta(provider.api_format, &event) {
                            translated.push_str(&delta);
                            on_delta(&delta, false);
                        }
                    }
                    Ok::<String, anyhow::Error>(translated)
                } else {
                    let response: Value = match provider.api_format {
                        ApiFormat::Responses => client.responses().create_byot(body).await,
                        ApiFormat::ChatCompletions => client.chat().create_byot(body).await,
                        ApiFormat::Completions => client.completions().create_byot(body).await,
                    }
                    .context("AI 请求失败")?;
                    let translated =
                        extract_output(provider.api_format, &response).context("AI 未返回文本")?;
                    on_delta(&translated, false);
                    Ok::<String, anyhow::Error>(translated)
                }
            },
        )
        .await
        .context("AI 请求超时")?
    })?;
    if translated.trim().is_empty() {
        bail!("AI 未返回文本");
    }
    cache.put(&key, &translated)?;
    Ok(translated)
}

fn render_prompt(template: &str, text: &str, target_language: &str) -> String {
    template
        .replace("{target_language}", target_language)
        .replace("{text}", text)
}

fn build_request_body(
    provider: &ProviderConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<Value> {
    let mut body = match provider.api_format {
        ApiFormat::Responses => json!({
            "model": provider.model,
            "instructions": system_prompt,
            "input": user_prompt,
            "temperature": provider.temperature,
            "max_output_tokens": provider.max_output_tokens,
            "stream": provider.stream,
        }),
        ApiFormat::ChatCompletions => json!({
            "model": provider.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt },
            ],
            "temperature": provider.temperature,
            "max_tokens": provider.max_output_tokens,
            "stream": provider.stream,
        }),
        ApiFormat::Completions => json!({
            "model": provider.model,
            "prompt": format!("{system_prompt}\n\n{user_prompt}"),
            "temperature": provider.temperature,
            "max_tokens": provider.max_output_tokens,
            "stream": provider.stream,
        }),
    };
    let object = body.as_object_mut().expect("请求体始终是 JSON 对象");
    merge_extra_params(object, &provider.extra_params)?;
    Ok(body)
}

fn merge_extra_params(
    body: &mut Map<String, Value>,
    extra_params: &BTreeMap<String, toml::Value>,
) -> Result<()> {
    for (key, value) in extra_params {
        body.insert(key.clone(), serde_json::to_value(value)?);
    }
    Ok(())
}

fn extract_output(format: ApiFormat, response: &Value) -> Option<String> {
    match format {
        ApiFormat::Responses => {
            if let Some(text) = response.get("output_text").and_then(Value::as_str) {
                return Some(text.to_owned());
            }
            let texts = response
                .get("output")?
                .as_array()?
                .iter()
                .flat_map(|item| {
                    item.get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>();
            (!texts.is_empty()).then(|| texts.join(""))
        }
        ApiFormat::ChatCompletions => response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ApiFormat::Completions => response
            .pointer("/choices/0/text")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn extract_stream_delta(format: ApiFormat, event: &Value) -> Option<String> {
    match format {
        ApiFormat::Responses => (event.get("type").and_then(Value::as_str)
            == Some("response.output_text.delta"))
        .then(|| event.get("delta").and_then(Value::as_str))
        .flatten()
        .map(str::to_owned),
        ApiFormat::ChatCompletions => event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ApiFormat::Completions => event
            .pointer("/choices/0/text")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn extract_stream_error(event: &Value) -> Option<&str> {
    event
        .get("error")
        .and_then(|error| error.get("message").or(Some(error)))
        .and_then(Value::as_str)
        .or_else(|| {
            (event.get("type").and_then(Value::as_str) == Some("error"))
                .then(|| event.get("message").and_then(Value::as_str))
                .flatten()
        })
        .or_else(|| {
            event
                .pointer("/response/error/message")
                .and_then(Value::as_str)
        })
}

fn api_format_name(format: ApiFormat) -> &'static str {
    match format {
        ApiFormat::Responses => "responses",
        ApiFormat::ChatCompletions => "chat_completions",
        ApiFormat::Completions => "completions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn renders_both_prompt_placeholders() {
        assert_eq!(
            render_prompt("Translate {text} to {target_language}", "hello", "zh-CN"),
            "Translate hello to zh-CN"
        );
    }

    #[test]
    fn appends_arbitrary_parameters() {
        let extra = BTreeMap::from([
            ("top_p".into(), toml::Value::Float(0.8)),
            ("seed".into(), toml::Value::Integer(42)),
        ]);
        let config: Config = toml::from_str(crate::config::DEFAULT_CONFIG).unwrap();
        let mut provider = config.active_provider().clone();
        provider.extra_params = extra;
        let body = build_request_body(&provider, "system", "user").unwrap();
        assert_eq!(body["top_p"], 0.8);
        assert_eq!(body["seed"], 42);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn extracts_all_supported_response_formats() {
        let responses = json!({"output": [{"content": [{"type": "output_text", "text": "one"}, {"type": "output_text", "text": "two"}]}]});
        let chat = json!({"choices": [{"message": {"content": "chat"}}]});
        let completions = json!({"choices": [{"text": "completion"}]});
        assert_eq!(
            extract_output(ApiFormat::Responses, &responses).as_deref(),
            Some("onetwo")
        );
        assert_eq!(
            extract_output(ApiFormat::ChatCompletions, &chat).as_deref(),
            Some("chat")
        );
        assert_eq!(
            extract_output(ApiFormat::Completions, &completions).as_deref(),
            Some("completion")
        );
    }

    #[test]
    fn extracts_all_supported_stream_deltas() {
        let responses = json!({"type": "response.output_text.delta", "delta": "one"});
        let chat = json!({"choices": [{"delta": {"content": "two"}}]});
        let completions = json!({"choices": [{"text": "three"}]});
        assert_eq!(
            extract_stream_delta(ApiFormat::Responses, &responses).as_deref(),
            Some("one")
        );
        assert_eq!(
            extract_stream_delta(ApiFormat::ChatCompletions, &chat).as_deref(),
            Some("two")
        );
        assert_eq!(
            extract_stream_delta(ApiFormat::Completions, &completions).as_deref(),
            Some("three")
        );
        assert_eq!(
            extract_stream_delta(
                ApiFormat::Responses,
                &json!({"type": "response.reasoning_summary_text.delta", "delta": "hidden"})
            ),
            None
        );
    }

    #[test]
    fn extracts_stream_errors() {
        assert_eq!(
            extract_stream_error(&json!({"type": "error", "message": "failed"})),
            Some("failed")
        );
        assert_eq!(
            extract_stream_error(
                &json!({"type": "response.failed", "response": {"error": {"message": "rejected"}}})
            ),
            Some("rejected")
        );
    }

    #[test]
    fn streams_chat_completions_incrementally() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = socket.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    let body: Value = serde_json::from_slice(&request[header_end + 4..]).unwrap();
                    assert_eq!(body["stream"], true);
                    break;
                }
            }

            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
                )
                .unwrap();
            for data in [
                r#"{"choices":[{"delta":{"content":"你"}}]}"#,
                r#"{"choices":[{"delta":{"content":"好"}}]}"#,
            ] {
                write!(socket, "data: {data}\n\n").unwrap();
                socket.flush().unwrap();
                thread::sleep(Duration::from_millis(20));
            }
            socket.write_all(b"data: [DONE]\n\n").unwrap();
        });

        let mut config: Config = toml::from_str(crate::config::DEFAULT_CONFIG).unwrap();
        let provider = &mut config.providers[0];
        provider.api_key = "test-key".into();
        provider.base_url = format!("http://{address}");
        provider.api_format = ApiFormat::ChatCompletions;
        provider.stream = true;
        provider.request_timeout_seconds = 5;
        let cache_path = std::env::temp_dir().join(format!(
            "fy-stream-test-{}-{}.sqlite3",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut deltas = Vec::new();
        let translated = translate(&config, &cache_path, "hello", |delta, from_cache| {
            deltas.push((delta.to_owned(), from_cache));
        })
        .unwrap();

        assert_eq!(translated, "你好");
        assert_eq!(deltas, [("你".into(), false), ("好".into(), false)]);
        let mut cached = None;
        let translated = translate(&config, &cache_path, "hello", |text, from_cache| {
            cached = Some((text.to_owned(), from_cache));
        })
        .unwrap();
        assert_eq!(translated, "你好");
        assert_eq!(cached, Some(("你好".into(), true)));
        server.join().unwrap();
        fs::remove_file(cache_path).unwrap();
    }
}
