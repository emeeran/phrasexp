//! Phase 7: AI text completion via a bring-your-own-key, OpenAI-compatible endpoint.
//! Configured in Settings (ai_enabled, ai_base_url, ai_api_key, ai_model).

use crate::domain::error::{DomainError, DomainResult};
use crate::domain::models::Settings;

/// Complete the prompt via the configured endpoint; returns the response text.
pub fn complete(settings: &Settings, prompt: &str) -> DomainResult<String> {
    if !settings.ai_enabled || settings.ai_api_key.is_empty() {
        return Err(DomainError::Validation(
            "AI not configured — enable it and set an API key in Settings".into(),
        ));
    }
    let base = settings.ai_base_url.trim_end_matches('/');
    let url = format!("{base}/chat/completions");
    let body = serde_json::json!({
        "model": settings.ai_model,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| DomainError::Validation(format!("http client: {e}")))?;
    let resp = client
        .post(&url)
        .bearer_auth(&settings.ai_api_key)
        .json(&body)
        .send()
        .map_err(|e| DomainError::Validation(format!("AI request: {e}")))?;
    let val: serde_json::Value = resp
        .json()
        .map_err(|e| DomainError::Validation(format!("AI response parse: {e}")))?;
    Ok(val["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}
