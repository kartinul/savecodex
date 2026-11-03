use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;

use crate::prompts::input_generation_prompt;

/// One piece of a multimodal prompt: text, or an image as (mime type, bytes).
pub enum Part {
    Text(String),
    Image(&'static str, Vec<u8>),
}

fn gemini_parts(parts: &[Part]) -> Vec<Value> {
    use base64::Engine;
    parts.iter().map(|p| match p {
        Part::Text(t) => json!({"text": t}),
        Part::Image(mime, bytes) => json!({"inline_data": {"mime_type": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes)}}),
    }).collect()
}

fn openai_parts(parts: &[Part]) -> Vec<Value> {
    use base64::Engine;
    parts.iter().map(|p| match p {
        Part::Text(t) => json!({"type": "text", "text": t}),
        Part::Image(mime, bytes) => json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", mime, base64::engine::general_purpose::STANDARD.encode(bytes))}}),
    }).collect()
}

#[derive(Clone, Debug)]
pub enum Provider {
    Gemini,
    OpenAiCompatible,
}

#[derive(Clone, Debug)]
pub struct AiConfig {
    pub provider: Provider,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: String,
}

impl AiConfig {
    pub fn from_env(prefix: &str) -> Result<Self> {
        let provider_str = env::var(format!("{}_PROVIDER", prefix))
            .unwrap_or_else(|_| "gemini".to_string());
        
        let provider = match provider_str.as_str() {
            "gemini" => Provider::Gemini,
            "openai_compatible" => Provider::OpenAiCompatible,
            _ => Provider::Gemini,
        };

        let model = env::var(format!("{}_MODEL", prefix))
            .unwrap_or_else(|_| "gemini-1.5-pro".to_string());

        match provider {
            Provider::Gemini => {
                let api_key = env::var("GEMINI_API_KEY").context("GEMINI_API_KEY must be set for Gemini provider")?;
                Ok(Self { provider, model, base_url: None, api_key })
            }
            Provider::OpenAiCompatible => {
                let api_key = env::var(format!("{}_API_KEY", prefix)).context(format!("{}_API_KEY must be set", prefix))?;
                let base_url = env::var(format!("{}_BASE_URL", prefix)).context(format!("{}_BASE_URL must be set", prefix))?;
                Ok(Self { provider, model, base_url: Some(base_url), api_key })
            }
        }
    }
}

async fn send_request(config: &AiConfig, parts: &[Part], json_only: bool) -> Result<String> {
    let client = Client::new();

    tracing::debug!("Contacting AI provider...");

    match config.provider {
        Provider::Gemini => {
            let models: Vec<&str> = config.model.split(',').collect();
            let mut last_err = None;

            for model in models {
                tracing::debug!("Trying Gemini model: {}", model.trim());
                let url = format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                    model.trim(), config.api_key
                );
                let mut body = json!({
                    "contents": [{
                        "parts": gemini_parts(parts)
                    }]
                });
                if json_only {
                    // thinking tokens count towards the output limit, so leave room for a full answer
                    body["generationConfig"] = json!({"responseMimeType": "application/json", "maxOutputTokens": 65536});
                }

                let resp = match client.post(&url).json(&body).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = Some(anyhow::anyhow!("Request failed for {}: {}", model.trim(), e));
                        continue;
                    }
                };
                
                if !resp.status().is_success() {
                    let status = resp.status();
                    let err_text = resp.text().await.unwrap_or_default();
                    let err_lower = err_text.to_lowercase();
                    if status == reqwest::StatusCode::UNAUTHORIZED || (status == reqwest::StatusCode::BAD_REQUEST && (err_lower.contains("api_key") || err_lower.contains("api key") || err_lower.contains("key invalid"))) {
                        anyhow::bail!("Invalid API Key");
                    }
                    last_err = Some(anyhow::anyhow!("HTTP error for {}: {} - {}", model.trim(), status, err_text));
                    continue;
                }

                let res: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(e) => {
                        last_err = Some(anyhow::anyhow!("JSON error for {}: {}", model.trim(), e));
                        continue;
                    }
                };
                
                // long answers can arrive split across several parts
                let text: Option<String> = res["candidates"][0]["content"]["parts"].as_array()
                    .map(|ps| ps.iter().filter_map(|p| p["text"].as_str()).collect::<String>())
                    .filter(|t| !t.is_empty());
                tracing::debug!("Received response from Gemini model {}", model.trim());
                tracing::trace!("Response text: {:?}", text);
                    
                if res["candidates"][0]["finishReason"].as_str() == Some("MAX_TOKENS") {
                    last_err = Some(anyhow::anyhow!("{} hit its output token limit (reply cut off)", model.trim()));
                    continue;
                }
                match text {
                    Some(t) => return Ok(t),
                    None => {
                        let finish_reason = res["candidates"][0]["finishReason"].as_str();
                        if finish_reason == Some("STOP") {
                            // Gemini returned empty content, which means the program needs NO input!
                            return Ok("".to_string());
                        } else {
                            last_err = Some(anyhow::anyhow!("Failed to extract text from {}. Raw: {}", model.trim(), serde_json::to_string_pretty(&res).unwrap_or_default()));
                            continue;
                        }
                    }
                }
            }
            
            anyhow::bail!("All specified Gemini models failed. Last error: {:?}", last_err)
        }
        Provider::OpenAiCompatible => {
            tracing::debug!("Trying OpenAI compatible model: {}", config.model);
            let url = format!("{}/chat/completions", config.base_url.as_ref().unwrap());
            let body = json!({
                "model": config.model,
                "messages": [{"role": "user", "content": openai_parts(parts)}]
            });

            let resp = client.post(&url)
                .bearer_auth(&config.api_key)
                .json(&body)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                let err_lower = err_text.to_lowercase();
                if status == reqwest::StatusCode::UNAUTHORIZED || err_lower.contains("invalid") || err_lower.contains("api_key") {
                    anyhow::bail!("Invalid API Key");
                }
                anyhow::bail!("HTTP error: {} - {}", status, err_text);
            }

            let res: Value = resp.json().await?;

            tracing::debug!("Received response from OpenAI compatible provider");
            let text = res["choices"][0]["message"]["content"].as_str();
            tracing::trace!("Response text: {:?}", text);

            text.map(|s| s.to_string())
                .context("Failed to extract text from OpenAI compatible response")
        }
    }
}

/// Ask the `CODE_*` model (the smart one, used by `solve`).
pub async fn ask_code(parts: &[Part]) -> Result<String> {
    send_request(&AiConfig::from_env("CODE")?, parts, false).await
}

/// Like `ask_code`, but Gemini is put in JSON mode so the reply is valid JSON.
pub async fn ask_code_json(parts: &[Part]) -> Result<String> {
    send_request(&AiConfig::from_env("CODE")?, parts, true).await
}

pub async fn generate_input(files: &[(&str, &str)], config: Option<&AiConfig>) -> Result<String> {
    let env_config;
    let final_config = match config {
        Some(c) => c,
        None => {
            env_config = AiConfig::from_env("INPUT")?;
            &env_config
        }
    };
    let prompt = input_generation_prompt(files);
    send_request(final_config, &[Part::Text(prompt)], false).await
}
