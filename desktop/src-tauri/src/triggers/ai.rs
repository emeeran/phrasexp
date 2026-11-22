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

/// The output contract every call carries as its final system message: models
/// love to chat ("Here's your rewritten text:"), quote-wrap answers, or wrap
/// plain prose in ``` fences. A text expander needs the bare transformed text —
/// this is the biggest single lever on response quality, and it applies to
/// every action and macro without touching stored templates.
pub(crate) const OUTPUT_CONTRACT: &str = "\
You are a text-expansion engine inside a desktop app. Return ONLY the final transformed \
text, ready to paste: no preamble (\"Here is…\"), no commentary, no surrounding quotes, \
and no markdown code fences unless the transformed text itself is code. Preserve the \
input's line breaks. If you cannot fulfill the request, return a one-line error \
prefixed with \"AI error:\" instead of an apology.";

/// Brand-voice instructions ride along as a system message on every call, with
/// the output contract appended after them.
fn build_messages(settings: &Settings, prompt: &str) -> Vec<serde_json::Value> {
    let mut messages = Vec::with_capacity(3);
    let voice = settings.ai_instructions.trim();
    if !voice.is_empty() {
        messages.push(serde_json::json!({ "role": "system", "content": voice }));
    }
    messages.push(serde_json::json!({ "role": "system", "content": OUTPUT_CONTRACT }));
    messages.push(serde_json::json!({ "role": "user", "content": prompt }));
    messages
}

/// Error for a non-success endpoint response: prefer the OpenAI-style
/// `{"error": {"message": ...}}` detail when present, else a truncated body.
/// A per-status hint turns the common failures into actionable messages.
fn endpoint_error(status: reqwest::StatusCode, raw: &str) -> DomainError {
    let detail = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| crate::domain::truncate(raw, 200));
    let hint = match status.as_u16() {
        401 | 403 => " — check your API key",
        404 => " — check the model name",
        413 => " — selection too long for this model",
        429 => " — rate limited, try again shortly",
        s if s >= 500 => " — provider outage (already retried)",
        _ => "",
    };
    DomainError::Validation(format!("AI endpoint {status}{hint}: {detail}"))
}

/// Marker appended when the model hit its token limit mid-answer — the user
/// needs to know the text was cut, not trust a silently truncated expansion.
pub(crate) const TRUNCATED_MARKER: &str =
    "\n\n[truncated: the response hit the token limit — raise Max tokens in AI settings]";

/// Defensive cleanup of a finished completion: trim whitespace and strip ONE
/// outer code fence (```lang\n…\n```) when the whole answer is fence-wrapped.
/// Fences *inside* the answer — actual code the model was asked to produce —
/// survive untouched. Pure — unit-tested.
pub(crate) fn sanitize_completion(text: &str) -> String {
    let trimmed = text.trim();
    let Some(inner) = trimmed
        .strip_prefix("```")
        .and_then(|r| r.strip_suffix("```"))
    else {
        return trimmed.to_string();
    };
    // Drop an optional language tag on the opening fence, then the newline after it.
    let body = inner.split_once('\n').map(|(_, b)| b).unwrap_or(inner);
    body.trim().to_string()
}

/// HTTP statuses worth an automatic retry: rate limits and transient provider
/// failures. Client errors (401/404/413) are deterministic — retrying burns time.
fn retryable(status: reqwest::StatusCode) -> bool {
    matches!(
        status.as_u16(),
        429 | 500 | 502 | 503 | 504
    )
}

/// Backoff before retry `attempt` (1-based): ~700ms, ~1.4s — quick enough to stay
/// inside the popup flow, long enough to clear a rate-limit window. Honors a
/// server-sent `Retry-After` (seconds) when present.
async fn backoff_delay(attempt: u32, retry_after: Option<f64>) {
    let secs = retry_after.unwrap_or(0.7 * f64::from(attempt));
    tokio::time::sleep(std::time::Duration::from_secs_f64(secs.min(10.0))).await;
}

/// Complete the prompt via the configured endpoint; returns the response text.
///
/// `async` so the HTTP wait yields the runtime thread instead of blocking it — the AI
/// call is the only network I/O in the expansion pipeline, and `reqwest::blocking` would
/// stall (or panic) when invoked from a Tauri async command. Callers bridge via
/// `tauri::async_runtime::block_on` where they are themselves synchronous.
pub async fn complete(settings: &Settings, prompt: &str) -> DomainResult<String> {
    let resp = start_completion(settings, prompt, false, std::time::Duration::from_secs(90)).await?;
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
    let mut out = sanitize_completion(content);
    if val["choices"][0]["finish_reason"] == "length" {
        out.push_str(TRUNCATED_MARKER);
    }
    Ok(out)
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
    let resp = start_completion(settings, prompt, true, std::time::Duration::from_secs(300)).await?;
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
    let mut truncated = false;
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
            if let Some(reason) = parse_sse_finish_reason(line) {
                truncated |= reason == "length";
            }
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
    // Sanitize only the final value — the streamed deltas stay raw while typing,
    // and the result view replaces them with this cleaned text.
    let mut out = sanitize_completion(&full);
    if truncated {
        out.push_str(TRUNCATED_MARKER);
    }
    Ok(out)
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

/// Extract the terminal `finish_reason` from an SSE data chunk (final chunk only;
/// earlier chunks carry null). Detects `length` so truncation can be surfaced.
fn parse_sse_finish_reason(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim_start();
    if data == "[DONE]" {
        return None;
    }
    let val = serde_json::from_str::<serde_json::Value>(data).ok()?;
    val["choices"][0]["finish_reason"]
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

/// Shared request preamble for `complete` / `complete_stream`: endpoint gates, URL,
/// body (`stream` set when requested), client with timeout, and the authed POST.
/// Response handling (error-body surfacing vs SSE streaming) stays with the caller.
async fn start_completion(
    settings: &Settings,
    prompt: &str,
    stream: bool,
    timeout: std::time::Duration,
) -> DomainResult<reqwest::Response> {
    let base = endpoint_gate(settings)?;
    let url = format!("{base}/chat/completions");
    // Brand-voice instructions + output contract ride along as system messages on
    // every call (see `build_messages`).
    let mut body = serde_json::json!({
        "model": settings.ai_model,
        "messages": build_messages(settings, prompt),
        "temperature": settings.ai_temperature,
    });
    if settings.ai_max_tokens > 0 {
        body["max_tokens"] = serde_json::Value::from(settings.ai_max_tokens);
    }
    if stream {
        body["stream"] = serde_json::Value::Bool(true);
    }
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| DomainError::Validation(format!("http client: {e}")))?;
    // Retry rate limits + transient provider failures (429/5xx) and network errors
    // twice before giving up — a silent retry beats a red toast for a blip.
    let mut retry_after_hdr = None;
    for attempt in 0..=2 {
        if attempt > 0 {
            backoff_delay(attempt, retry_after_hdr.take()).await;
        }
        let sent = client
            .post(&url)
            .bearer_auth(&settings.ai_api_key)
            .json(&body)
            .send()
            .await;
        match sent {
            Ok(resp) if retryable(resp.status()) => {
                retry_after_hdr = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<f64>().ok());
                if attempt == 2 {
                    // Out of retries: surface the final status + body (same detail
                    // as a one-shot failure) — endpoint_error notes the retries.
                    let status = resp.status();
                    let raw = resp.text().await.unwrap_or_default();
                    return Err(endpoint_error(status, &raw));
                }
            }
            Ok(resp) => return Ok(resp),
            Err(e) if attempt == 2 => {
                return Err(DomainError::Validation(format!("AI request: {e}")))
            }
            Err(_) => {} // transient network error — retry
        }
    }
    unreachable!("retry loop always returns")
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

    /// Serve a SEQUENCE of canned responses (one per connection) on an ephemeral
    /// loopback port — enough to exercise the retry path. Returns base URL.
    fn spawn_scripted_server(script: &[(&'static str, &'static str)]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let script: Vec<(String, String)> = script
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for (status_line, body) in &script {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
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
        // 5xx is retried, so the script must outlast the retry budget.
        let base = spawn_scripted_server(&[
            ("HTTP/1.1 502 Bad Gateway", "<html>bad gateway</html>"),
            ("HTTP/1.1 502 Bad Gateway", "<html>bad gateway</html>"),
            ("HTTP/1.1 502 Bad Gateway", "<html>bad gateway</html>"),
        ]);
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
        // No brand voice — but the output contract still rides along as a system
        // message (it's what keeps answers paste-ready).
        assert!(
            req.contains("text-expansion engine"),
            "output contract missing: {req}"
        );
    }

    #[test]
    fn sanitize_strips_one_outer_code_fence() {
        assert_eq!(sanitize_completion("  hello  "), "hello");
        assert_eq!(sanitize_completion("plain text"), "plain text");
        // Fence-wrapped answer → bare text.
        assert_eq!(
            sanitize_completion("```rust\nfn main() {}\n```"),
            "fn main() {}"
        );
        assert_eq!(sanitize_completion("```\njust fenced\n```"), "just fenced");
        // Fences INSIDE the answer are content — untouched.
        let inner = "Use this:\n```py\nprint(1)\n```\nthanks";
        assert_eq!(sanitize_completion(inner), inner);
    }

    #[test]
    fn build_messages_always_carry_the_output_contract() {
        let mut s = Settings::defaults();
        s.ai_instructions = "Be terse.".into();
        let msgs = build_messages(&s, "do it");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["content"], "Be terse.");
        assert!(msgs[1]["content"]
            .as_str()
            .unwrap()
            .contains("text-expansion engine"));
        assert_eq!(msgs[2]["content"], "do it");
        // No voice → contract is the only system message.
        s.ai_instructions.clear();
        let msgs = build_messages(&s, "do it");
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0]["content"].as_str().unwrap().contains("text-expansion engine"));
    }

    #[test]
    fn request_carries_temperature_and_max_tokens() {
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
        s.ai_temperature = 0.3;
        s.ai_max_tokens = 512;
        tauri::async_runtime::block_on(complete(&s, "hello")).unwrap();
        let req = handle.join().unwrap();
        assert!(req.contains("\"temperature\":0.3"), "no temperature: {req}");
        assert!(req.contains("\"max_tokens\":512"), "no max_tokens: {req}");
    }

    #[test]
    fn max_tokens_zero_is_omitted_from_request() {
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
        assert_eq!(s.ai_max_tokens, 0);
        tauri::async_runtime::block_on(complete(&s, "hello")).unwrap();
        let req = handle.join().unwrap();
        assert!(!req.contains("max_tokens"), "unexpected cap: {req}");
    }

    #[test]
    fn rate_limit_is_retried_then_succeeds() {
        let ok_body = r#"{"choices":[{"message":{"content":"recovered"}}]}"#;
        let base = spawn_scripted_server(&[
            ("HTTP/1.1 429 Too Many Requests", r#"{"error":{"message":"slow down"}}"#),
            ("HTTP/1.1 503 Service Unavailable", "<html>down</html>"),
            ("HTTP/1.1 200 OK", ok_body),
        ]);
        let out = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi")).unwrap();
        assert_eq!(out, "recovered");
    }

    #[test]
    fn exhausted_retries_surface_the_last_status() {
        let base = spawn_scripted_server(&[
            ("HTTP/1.1 500 Internal Server Error", "err"),
            ("HTTP/1.1 500 Internal Server Error", "err"),
            ("HTTP/1.1 500 Internal Server Error", "err"),
        ]);
        let err = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("500"), "missing status: {err}");
        assert!(err.contains("already retried"), "missing hint: {err}");
    }

    #[test]
    fn client_errors_are_not_retried() {
        // Scripted server serves only ONE response: a second request would hang
        // (then fail on client timeout) if 401 were retried.
        let base = spawn_server(
            "HTTP/1.1 401 Unauthorized",
            r#"{"error":{"message":"bad key"}}"#,
        );
        let err = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("check your API key"), "unexpected: {err}");
    }

    #[test]
    fn length_finish_reason_appends_truncation_marker() {
        let base = spawn_server(
            "HTTP/1.1 200 OK",
            r#"{"choices":[{"message":{"content":"half a sent"},"finish_reason":"length"}]}"#,
        );
        let out = tauri::async_runtime::block_on(complete(&ai_settings(&base), "hi")).unwrap();
        assert!(out.starts_with("half a sent"), "unexpected: {out}");
        assert!(out.contains("truncated"), "no marker: {out}");
    }

    #[test]
    fn sse_finish_reason_is_parsed() {
        assert_eq!(
            parse_sse_finish_reason(
                r#"data: {"choices":[{"delta":{},"finish_reason":"length"}]}"#
            ),
            Some("length".into())
        );
        assert_eq!(
            parse_sse_finish_reason(r#"data: {"choices":[{"delta":{"content":"x"}}]}"#),
            None
        );
        assert_eq!(parse_sse_finish_reason("data: [DONE]"), None);
    }
}
