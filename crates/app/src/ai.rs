use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::env;

use crate::prompts::{code_generation_prompt, input_generation_prompt};

enum Provider {
    Gemini,
    OpenAiCompatible,
}

struct AiConfig {
    provider: Provider,
    model: String,
    base_url: Option<String>,
    api_key: String,
}

impl AiConfig {
    fn from_env(prefix: &str) -> Result<Self> {
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

async fn send_request(config: &AiConfig, prompt: &str) -> Result<String> {
    let client = Client::new();

    tracing::debug!("Contacting AI provider...");
    tracing::trace!("Prompt: {}", prompt);

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
                let body = json!({
                    "contents": [{
                        "parts": [{"text": prompt}]
                    }]
                });

                let resp = match client.post(&url).json(&body).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = Some(anyhow::anyhow!("Request failed for {}: {}", model.trim(), e));
                        continue;
                    }
                };
                
                if !resp.status().is_success() {
                    last_err = Some(anyhow::anyhow!("HTTP error for {}: {}", model.trim(), resp.status()));
                    continue;
                }

                let res: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(e) => {
                        last_err = Some(anyhow::anyhow!("JSON error for {}: {}", model.trim(), e));
                        continue;
                    }
                };
                
                let text = res["candidates"][0]["content"]["parts"][0]["text"].as_str();
                tracing::debug!("Received response from Gemini model {}", model.trim());
                tracing::trace!("Response text: {:?}", text);
                    
                match text {
                    Some(t) => return Ok(t.to_string()),
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
                "messages": [{"role": "user", "content": prompt}]
            });

            let res: Value = client.post(&url)
                .bearer_auth(&config.api_key)
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            tracing::debug!("Received response from OpenAI compatible provider");
            let text = res["choices"][0]["message"]["content"].as_str();
            tracing::trace!("Response text: {:?}", text);

            text.map(|s| s.to_string())
                .context("Failed to extract text from OpenAI compatible response")
        }
    }
}

pub async fn generate_code(system_prompt: &str, question: &str) -> Result<String> {
    let config = AiConfig::from_env("CODE")?;
    let prompt = code_generation_prompt(system_prompt, question);
    send_request(&config, &prompt).await
}

pub async fn generate_input(files: &[(&str, &str)]) -> Result<String> {
    let config = AiConfig::from_env("INPUT")?;
    let prompt = input_generation_prompt(files);
    send_request(&config, &prompt).await
}
