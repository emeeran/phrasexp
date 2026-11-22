//! AI text completion via a bring-your-own-key, OpenAI-compatible endpoint.
//! Configured in Settings (ai_enabled, ai_base_url, ai_api_key, ai_model).

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::Settings;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared preflight for `complete`/`complete_stream`: reject unconfigured AI or a
/// plaintext remote endpoint, and return the trimmed base URL.
/// Plaintext remote endpoints would send the Bearer key in the clear (AUDIT 117).
/// Loopback http stays allowed for local model servers (Ollama, GPT4All).
fn endpoint_gate(settings: &Settings) -> DomainResult<&str> {
    if !settings.ai_enabled || settings.ai_api_key.is_empty() {
        return Err(DomainError::Validation(
            "AI not configured — enable it and set an API key in Settings".into(),
        ));
    }
    let base = settings.ai_base_url.trim_end_matches('/');
    if !base.starts_with("https://") && !is_loopback_url(base) {
        return Err(DomainError::Validation(
            "AI base URL must use https:// (plain http is only allowed for localhost)".into(),
        ));
    }
    Ok(base)
}

/// Brand-voice instructions ride along as a system message on every call.
fn build_messages(settings: &Settings, prompt: &str) -> Vec<serde_json::Value> {
    let mut messages = Vec::with_capacity(2);
    let voice = settings.ai_instructions.trim();
    if !voice.is_empty() {
        messages.push(serde_json::json!({ "role": "system", "content": voice }));
    }
    messages.push(serde_json::json!({ "role": "user", "content": prompt }));
    messages
}

/// Error for a non-success endpoint response: prefer the OpenAI-style
/// `{"error": {"message": ...}}` detail when present, else a truncated body.
fn endpoint_error(status: reqwest::StatusCode, raw: &str) -> DomainError {
    let detail = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| truncate(raw, 200));
    DomainError::Validation(format!("AI endpoint {status}: {detail}"))
}

/// Complete the prompt via the configured endpoint; returns the response text.
///
/// `async` so the HTTP wait yields the runtime thread instead of blocking it — the AI
/// call is the only network I/O in the expansion pipeline, and `reqwest::blocking` would
/// stall (or panic) when invoked from a Tauri async command. Callers bridge via
/// `tauri::async_runtime::block_on` where they are themselves synchronous.
pub async fn complete(settings: &Settings, prompt: &str) -> DomainResult<String> {
    let base = endpoint_gate(settings)?;
    let url = format!("{base}/chat/completions");
    // Brand-voice instructions ride along as a system message on every call (see
    // `build_messages`).
    let body = serde_json::json!({
        "model": settings.ai_model,
        "messages": build_messages(settings, prompt),
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| DomainError::Validation(format!("http client: {e}")))?;
    let resp = client
        .post(&url)
        .bearer_auth(&settings.ai_api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| DomainError::Validation(format!("AI request: {e}")))?;
    let status = resp.status();
    // Read as text first so a non-JSON error body (proxy HTML, plain text) can be
    // surfaced verbatim instead of turning into a parse error.
    let raw = resp
        .text()
        .await
        .map_err(|e| DomainError::Validation(format!("AI response read: {e}")))?;
    if !status.is_success() {
        return Err(endpoint_error(status, &raw));
    }
    let val: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| DomainError::Validation(format!("AI response parse: {e}")))?;
    let content = val["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            DomainError::Validation(
                "AI response missing content — no choices[0].message.content".into(),
            )
        })?;
    Ok(content.to_string())
}

/// Stream the completion via the configured endpoint, invoking `on_delta` with each
/// incremental content chunk as it arrives (SSE `chat/completions` streaming). Returns
/// the full response text. `cancel`, when set mid-stream, stops at the next chunk —
/// the caller decides what a cancelled partial is worth. Shares the endpoint gates
/// and system-message handling with `complete`.
pub async fn complete_stream<F: for<'a> FnMut(&'a str)>(
    settings: &Settings,
    prompt: &str,
    cancel: &AtomicBool,
    mut on_delta: F,
) -> DomainResult<String> {
    let base = endpoint_gate(settings)?;
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": settings.ai_model,
        "messages": build_messages(settings, prompt),
        "stream": true,
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| DomainError::Validation(format!("http client: {e}")))?;
    let resp = client
        .post(&url)
        .bearer_auth(&settings.ai_api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| DomainError::Validation(format!("AI request: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        // Read as text so a non-JSON error body surfaces verbatim (same as `complete`).
        let raw = resp
            .text()
            .await
            .unwrap_or_else(|_| "unreadable error body".into());
        return Err(endpoint_error(status, &raw));
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut full = String::new();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return Err(DomainError::Validation(CANCELLED_MSG.into()));
        }
        let chunk = chunk.map_err(|e| DomainError::Validation(format!("AI response read: {e}")))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        // SSE events are newline-delimited; drain complete lines, keeping the tail
        // as a partial-line carry.
        while let Some(idx) = buf.find('\n') {
            let line: String = buf.drain(..=idx).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(delta) = parse_sse_delta(line) {
                if !delta.is_empty() {
                    on_delta(&delta);
                    full.push_str(&delta);
                }
            }
        }
    }
    if full.is_empty() {
        return Err(DomainError::Validation(
            "AI returned an empty response — nothing to replace with".into(),
        ));
    }
    Ok(full)
}

/// Message carried by the cancellation error from `complete_stream`; callers compare
/// against it to distinguish a user cancel from a real failure.
pub const CANCELLED_MSG: &str = "cancelled";

/// Extract the content delta from one SSE data line. `data: [DONE]` and everything
/// that isn't a well-formed chat chunk yields None. Pure — unit-tested without a server.
fn parse_sse_delta(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim_start();
    if data == "[DONE]" {
        return None;
    }
    let val = serde_json::from_str::<serde_json::Value>(data).ok()?;
    val["choices"][0]["delta"]["content"]
        .as_str()
        .map(str::to_string)
}

/// True when the URL host is loopback — plaintext http to a local model server
/// (Ollama, GPT4All) stays allowed while remote http is refused.
fn is_loopback_url(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    // Bracketed IPv6 literal — take up to ']' so port splitting can't clip it.
    let host = if let Some(stripped) = rest.strip_prefix('[') {
        let end = stripped.find(']').unwrap_or(stripped.len());
        &stripped[..end]
    } else {
        rest.split(['/', '?', ':']).next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ai_settings(base_url: &str) -> Settings {
        let mut s = Settings::defaults();
        s.ai_enabled = true;
        s.ai_api_key = "test-key".into();
        s.ai_base_url = base_url.to_string();
        s
    }

    /// Serve one canned HTTP response on an ephemeral loopback port; returns base URL.
    fn spawn_server(status_line: &'static str, body: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn http_error_status_becomes_err() {
        let base = spawn_server(
            "HTTP/1.1 401 Unauthorized",
            r#"{"error":{"message":"invalid api key"}}"#,
        );
        let err = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("401"), "missing status: {err}");
        assert!(err.contains("invalid api key"), "missing detail: {err}");
    }

    #[test]
    fn server_error_with_non_json_body_is_err() {
        let base = spawn_server("HTTP/1.1 502 Bad Gateway", "<html>bad gateway</html>");
        let err = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("502"), "missing status: {err}");
        assert!(err.contains("bad gateway"), "missing body: {err}");
    }

    #[test]
    fn ok_response_without_content_is_err() {
        let base = spawn_server("HTTP/1.1 200 OK", "{}");
        let err = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing content"), "unexpected: {err}");
    }

    #[test]
    fn ok_response_parses_content() {
        let base = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"choices":[{"message":{"content":"hello"}}]}"#,
        );
        let out = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi")).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn remote_plaintext_url_is_rejected() {
        // Rejected before any network I/O — no server, no DNS needed.
        let err = tauri::async_runtime::block_on(complete(
            &ai_settings("http://api.example.com/v1"),
            "hi",
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("https"), "unexpected: {err}");
    }

    #[test]
    fn loopback_and_https_urls_pass_scheme_gate() {
        assert!(is_loopback_url("http://127.0.0.1:11434/v1"));
        assert!(is_loopback_url("http://localhost:8080"));
        assert!(is_loopback_url("http://[::1]:11434"));
        assert!(!is_loopback_url("http://api.groq.com/openai/v1"));
        assert!(!is_loopback_url("https://api.groq.com/openai/v1"));
    }

    #[test]
    fn brand_voice_instructions_become_system_message() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            req
        });
        let mut s = ai_settings(&format!("http://{addr}"));
        s.ai_instructions = "Always write in British English.".into();
        tauri::async_runtime::block_on(complete(&s, "hello")).unwrap();
        let req = handle.join().unwrap();
        // Key order within the JSON body is alphabetical (serde_json), so assert
        // on presence of the role marker and the instruction text, not adjacency.
        assert!(
            req.contains("\"role\":\"system\""),
            "no system message: {req}"
        );
        assert!(
            req.contains("Always write in British English."),
            "instruction text missing: {req}"
        );
        assert!(
            req.contains("\"content\":\"hello\""),
            "user prompt missing: {req}"
        );
    }

    #[test]
    fn sse_delta_parsing() {
        let chunk = r#"data: {"choices":[{"delta":{"content":"He"}}]}"#;
        assert_eq!(parse_sse_delta(chunk).as_deref(), Some("He"));
        assert_eq!(parse_sse_delta("data: [DONE]"), None);
        // Comment lines and keep-alives carry no delta.
        assert_eq!(parse_sse_delta(": keep-alive"), None);
        assert_eq!(parse_sse_delta("event: ping"), None);
        // Malformed JSON is skipped, not an error.
        assert_eq!(parse_sse_delta("data: {oops"), None);
        // Delta without content (role-only first chunk) is None, not empty-string.
        assert_eq!(
            parse_sse_delta(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#),
            None
        );
    }

    #[test]
    fn stream_collects_deltas_in_order() {
        use std::io::{Read, Write};
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Drain the request first — replying before reading resets the
                // connection mid-send on some stacks.
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{sse}",
                    sse.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        let mut seen = Vec::new();
        let out = tauri::async_runtime::block_on(complete_stream(
            &ai_settings(&format!("http://{addr}")),
            "hi",
            &AtomicBool::new(false),
            |d| seen.push(d.to_string()),
        ))
        .unwrap();
        assert_eq!(out, "Hello");
        assert_eq!(seen, vec!["Hel".to_string(), "lo".to_string()]);
    }

    #[test]
    fn stream_cancel_stops_before_finish() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Drain the request first (see stream_collects_deltas_in_order).
                let mut req = [0u8; 1024];
                let _ = stream.read(&mut req);
                // Two SSE events; the client should bail after the first chunk.
                let body = "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n";
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.read(&mut [0u8; 16]);
            }
        });
        let cancel = AtomicBool::new(true);
        let err = tauri::async_runtime::block_on(complete_stream(
            &ai_settings(&format!("http://{addr}")),
            "hi",
            &cancel,
            |_| {},
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains(CANCELLED_MSG), "unexpected: {err}");
    }

    #[test]
    fn stream_http_error_is_err() {
        let base = spawn_server(
            "HTTP/1.1 401 Unauthorized",
            r#"{"error":{"message":"bad key"}}"#,
        );
        let err = tauri::async_runtime::block_on(complete_stream(
            &ai_settings(&base),
            "hi",
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("401"), "missing status: {err}");
    }

    #[test]
    fn no_instructions_means_no_system_message() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            req
        });
        let s = ai_settings(&format!("http://{addr}"));
        assert!(s.ai_instructions.is_empty());
        tauri::async_runtime::block_on(complete(&s, "hello")).unwrap();
        let req = handle.join().unwrap();
        assert!(!req.contains("system"), "unexpected system message: {req}");
    }
}
