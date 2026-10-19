//! Conversations whose every answer is one JSON object: what drafts tests and explains failed
//! runs. Same providers, keys and errors as checks by meaning.

use base64::Engine;
use serde_json::{Value, json};

use crate::{Client, MaskedImage, Provider, VisionError};

/// Longest answer a model may write, in tokens: a whole test fits.
const MAX_TOKENS: u32 = 8192;

/// One turn of a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// The model said it; otherwise JEVision did, for the person.
    pub from_model: bool,
    pub text: String,
    /// Pictures shown before the text, each after its label. Private parts already covered.
    pub pictures: Vec<(String, MaskedImage)>,
}

impl Message {
    pub fn asked(text: impl Into<String>) -> Self {
        Self {
            from_model: false,
            text: text.into(),
            pictures: Vec::new(),
        }
    }

    pub fn answered(text: impl Into<String>) -> Self {
        Self {
            from_model: true,
            ..Self::asked(text)
        }
    }
}

impl Client {
    /// The model's next answer in the conversation `messages`, which follows `system`: one JSON
    /// object, of the shape `schema` where the provider can enforce one.
    pub fn ask(
        &self,
        system: &str,
        messages: &[Message],
        schema: &Value,
    ) -> Result<Value, VisionError> {
        let c = &self.config;
        if c.model.trim().is_empty() {
            return Err(VisionError::NoModel);
        }
        if c.provider.needs_key() && c.key().is_none() {
            return Err(VisionError::NoKey);
        }
        let (url, body) = request(
            c.provider,
            c.model.trim(),
            c.address(),
            system,
            messages,
            schema,
        );
        let (status, text) = self.send(&url, Some(&body))?;
        if status != 200 {
            return Err(self.http_error(status, &text));
        }
        let mask = |e| match e {
            VisionError::BadAnswer { answer } => VisionError::BadAnswer {
                answer: self.excerpt(&answer),
            },
            VisionError::Refused { message } => VisionError::Refused {
                message: self.excerpt(&message),
            },
            other => other,
        };
        let answer = answer(c.provider, &text).map_err(mask)?;
        match c.key() {
            // A model that repeats the key gets it masked, wherever it put it.
            Some(key) if answer.to_string().contains(key) => {
                let masked = answer.to_string().replace(key, crate::MASK);
                Ok(serde_json::from_str(&masked).unwrap_or(answer))
            }
            _ => Ok(answer),
        }
    }
}

/// URL and JSON body that continue `messages`.
fn request(
    provider: Provider,
    model: &str,
    base: &str,
    system: &str,
    messages: &[Message],
    schema: &Value,
) -> (String, Value) {
    let b64 = |m: &MaskedImage| base64::engine::general_purpose::STANDARD.encode(m.png());
    let system =
        format!("{system}\n\nAnswer with one JSON object only, of this JSON Schema:\n{schema}");
    match provider {
        Provider::Anthropic => {
            let messages: Vec<Value> = messages
                .iter()
                .map(|m| {
                    let mut content = Vec::new();
                    for (label, picture) in &m.pictures {
                        content.push(json!({ "type": "text", "text": label }));
                        content.push(json!({
                            "type": "image",
                            "source": { "type": "base64", "media_type": "image/png", "data": b64(picture) }
                        }));
                    }
                    content.push(json!({ "type": "text", "text": m.text }));
                    json!({ "role": if m.from_model { "assistant" } else { "user" }, "content": content })
                })
                .collect();
            let body = json!({
                "model": model,
                "max_tokens": MAX_TOKENS,
                "temperature": 0,
                "system": system,
                "tools": [{
                    "name": "answer",
                    "description": "Give the answer.",
                    "input_schema": schema
                }],
                "tool_choice": { "type": "tool", "name": "answer" },
                "messages": messages
            });
            (format!("{base}/v1/messages"), body)
        }
        Provider::Gemini => {
            let contents: Vec<Value> = messages
                .iter()
                .map(|m| {
                    let mut parts = Vec::new();
                    for (label, picture) in &m.pictures {
                        parts.push(json!({ "text": label }));
                        parts.push(json!({ "inlineData": { "mimeType": "image/png", "data": b64(picture) } }));
                    }
                    parts.push(json!({ "text": m.text }));
                    json!({ "role": if m.from_model { "model" } else { "user" }, "parts": parts })
                })
                .collect();
            let body = json!({
                "systemInstruction": { "parts": [{ "text": system }] },
                "contents": contents,
                "generationConfig": {
                    "temperature": 0,
                    "maxOutputTokens": MAX_TOKENS,
                    "responseMimeType": "application/json"
                }
            });
            let model = model.strip_prefix("models/").unwrap_or(model);
            (
                format!("{base}/v1beta/models/{model}:generateContent"),
                body,
            )
        }
        Provider::Ollama => {
            let mut list = vec![json!({ "role": "system", "content": system })];
            list.extend(messages.iter().map(|m| {
                let labels: Vec<&str> = m.pictures.iter().map(|(l, _)| l.as_str()).collect();
                let text = if labels.is_empty() {
                    m.text.clone()
                } else {
                    format!("Pictures in order: {}\n{}", labels.join(" "), m.text)
                };
                let images: Vec<String> = m.pictures.iter().map(|(_, p)| b64(p)).collect();
                json!({
                    "role": if m.from_model { "assistant" } else { "user" },
                    "content": text,
                    "images": images
                })
            }));
            let body = json!({
                "model": model,
                "stream": false,
                "format": schema,
                "options": { "temperature": 0 },
                "messages": list
            });
            (format!("{base}/api/chat"), body)
        }
        Provider::OpenAi | Provider::LmStudio | Provider::OpenAiCompatible => {
            let mut list = vec![json!({ "role": "system", "content": system })];
            list.extend(messages.iter().map(|m| {
                if m.from_model {
                    return json!({ "role": "assistant", "content": m.text });
                }
                let mut content = Vec::new();
                for (label, picture) in &m.pictures {
                    content.push(json!({ "type": "text", "text": label }));
                    content.push(json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:image/png;base64,{}", b64(picture)) }
                    }));
                }
                content.push(json!({ "type": "text", "text": m.text }));
                json!({ "role": "user", "content": content })
            }));
            let mut body = json!({
                "model": model,
                "messages": list,
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "answer", "strict": false, "schema": schema }
                }
            });
            // Reasoning models at OpenAI take only the default temperature.
            if provider != Provider::OpenAi {
                body["temperature"] = json!(0);
            }
            (format!("{base}/chat/completions"), body)
        }
    }
}

/// The JSON object in a successful response body.
fn answer(provider: Provider, body: &str) -> Result<Value, VisionError> {
    let bad = |text: &str| VisionError::BadAnswer {
        answer: text.to_owned(),
    };
    let v: Value = serde_json::from_str(body).map_err(|_| bad(body))?;
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let said = match provider {
        Provider::Anthropic => {
            let content = v["content"].as_array().cloned().unwrap_or_default();
            let said: String = content
                .iter()
                .filter(|c| c["type"] == "text")
                .map(|c| text(&c["text"]))
                .collect();
            if v["stop_reason"] == "refusal" {
                return Err(VisionError::Refused { message: said });
            }
            if let Some(tool) = content.iter().find(|c| c["type"] == "tool_use") {
                return match &tool["input"] {
                    object @ Value::Object(_) => Ok(object.clone()),
                    other => Err(bad(&other.to_string())),
                };
            }
            said
        }
        Provider::Gemini => {
            if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
                return Err(VisionError::Refused {
                    message: reason.to_owned(),
                });
            }
            let candidate = &v["candidates"][0];
            let said: String = candidate["content"]["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| text(&p["text"]))
                .collect();
            if said.trim().is_empty()
                && let Some(reason) = candidate["finishReason"].as_str()
                && reason != "STOP"
            {
                return Err(VisionError::Refused {
                    message: reason.to_owned(),
                });
            }
            said
        }
        Provider::Ollama => text(&v["message"]["content"]),
        Provider::OpenAi | Provider::LmStudio | Provider::OpenAiCompatible => {
            let message = &v["choices"][0]["message"];
            if let Some(refusal) = message["refusal"].as_str() {
                return Err(VisionError::Refused {
                    message: refusal.to_owned(),
                });
            }
            text(&message["content"])
        }
    };
    object_in(&said).ok_or_else(|| bad(&said))
}

/// The JSON object in a model's text: plain, or wrapped in prose or a code fence.
pub(crate) fn object_in(said: &str) -> Option<Value> {
    serde_json::from_str::<Value>(said.trim())
        .ok()
        .or_else(|| {
            let start = said.find('{')?;
            let end = said.rfind('}')?;
            serde_json::from_str(said.get(start..=end)?).ok()
        })
        .filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, TIMEOUT};

    const KEY: &str = "sk-test-0123456789abcdef";

    fn conversation() -> Vec<Message> {
        let picture = MaskedImage::from_png(b"\x89PNG shot", &[]).unwrap();
        vec![
            Message {
                pictures: vec![("Screenshot:".into(), picture)],
                ..Message::asked("Why did it fail?")
            },
            Message::answered(r#"{"tool":{"server":"s","name":"t","arguments":{}}}"#),
            Message::asked("The tool said: fine."),
        ]
    }

    #[test]
    fn every_provider_gets_the_whole_conversation_and_the_shape() {
        let schema = json!({ "type": "object", "properties": { "x": { "type": "string" } } });
        for provider in Provider::ALL {
            let (url, body) = request(
                provider,
                "m",
                "http://localhost:9",
                "Be brief.",
                &conversation(),
                &schema,
            );
            let text = body.to_string();
            assert!(url.starts_with("http://localhost:9/"), "{provider:?}");
            for part in [
                "Be brief.",
                "Why did it fail?",
                "The tool said: fine.",
                "properties",
            ] {
                assert!(text.contains(part), "{provider:?} lacks {part}");
            }
            let shot = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG shot");
            assert!(text.contains(&shot), "{provider:?} lacks the picture");
            let roles = text.matches("\"assistant\"").count() + text.matches("\"model\"").count();
            assert!(roles >= 1, "{provider:?} lost who said what");
        }
    }

    #[test]
    fn answers_are_read_from_each_provider() {
        let wanted = json!({ "answer": { "verdict": "appBug" } });
        let said = serde_json::to_string(&wanted.to_string()).unwrap();
        let bodies = [
            (
                Provider::Anthropic,
                format!(r#"{{"content":[{{"type":"tool_use","input":{wanted}}}]}}"#),
            ),
            (
                Provider::Gemini,
                format!(r#"{{"candidates":[{{"content":{{"parts":[{{"text":{said}}}]}}}}]}}"#),
            ),
            (
                Provider::Ollama,
                format!(r#"{{"message":{{"content":{said}}}}}"#),
            ),
            (
                Provider::OpenAi,
                format!(r#"{{"choices":[{{"message":{{"content":{said}}}}}]}}"#),
            ),
        ];
        for (provider, body) in bodies {
            assert_eq!(answer(provider, &body).unwrap(), wanted, "{provider:?}");
        }
        assert_eq!(
            object_in("Here you go:\n```json\n{\"a\": 1}\n```"),
            Some(json!({ "a": 1 }))
        );
        assert_eq!(object_in("[1, 2]"), None);
        assert!(matches!(
            answer(
                Provider::OpenAi,
                r#"{"choices":[{"message":{"refusal":"no"}}]}"#
            ),
            Err(VisionError::Refused { .. })
        ));
    }

    #[test]
    fn missing_settings_fail_before_anything_is_sent() {
        let client = |provider, model: &str, key: Option<&str>| {
            Client::new(
                Config {
                    provider,
                    model: model.into(),
                    address: "http://127.0.0.1:9".into(),
                    key: key.map(Into::into),
                },
                TIMEOUT,
            )
        };
        let schema = json!({ "type": "object" });
        assert_eq!(
            client(Provider::Anthropic, "m", None).ask("", &[], &schema),
            Err(VisionError::NoKey)
        );
        assert_eq!(
            client(Provider::Ollama, " ", None).ask("", &[], &schema),
            Err(VisionError::NoModel)
        );
        assert!(!format!("{:?}", client(Provider::OpenAi, "m", Some(KEY)).config()).contains(KEY));
    }
}
