//! Asks vision models whether a screenshot shows what a test expects.
//!
//! [`Client`] talks to one provider over blocking HTTP(S): Anthropic, OpenAI, Google Gemini,
//! JEV (TypeSafe AI), Ollama, LM Studio or any server with an OpenAI-compatible API. Answers are requested in a
//! fixed JSON shape (`passed`, `reason`) wherever the provider can enforce one, and read
//! leniently where it cannot. The API key is sent only in a request header; error messages and
//! answers have it masked. Pictures go out only as [`MaskedImage`]s, private areas painted
//! over first.

mod ask;
pub mod image;
mod mask;

pub use ask::Message;
pub use mask::{MASK_COLOUR, MaskedImage, PixelArea, VideoMask, VideoWindow, video_mask};

use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use base64::Engine;
use jevision_core::Rect;
use serde_json::{Value, json};

/// How long one question may take, including a local model loading into memory.
pub const TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Shown instead of the API key wherever it would otherwise appear.
const MASK: &str = "••••";
/// Longest piece of a server message or a model answer kept for an error.
const EXCERPT: usize = 300;

/// The picture [`Client::test`] sends: left half red, right half blue.
const TEST_PICTURE: &[u8] = include_bytes!("test-picture.png");
const TEST_EXPECTATION: &str = "The left half of the picture is red and the right half is blue.";

const INSTRUCTIONS: &str = "You check screenshots for automated tests of app interfaces. \
Decide whether the screenshot shows what is expected. When a reference picture is given, it \
shows how the screen looked when the test was made: judge whether the screenshot means the \
same — the same screen, state and content — and ignore differences in exact pixels, fonts, \
colours, positions, times and dates unless the expectation mentions them. Flat boxes of one \
colour hide private parts of the screen on purpose; ignore them. Answer with JSON only: \
{\"passed\": true or false, \"reason\": \"one short sentence on what you see that decides it\"}. \
Write the reason in the language of the expectation.";

/// For JEV, which answers with the probability of "yes" instead of a verdict in JSON.
const JEV_INSTRUCTIONS: &str = "You check screenshots for automated tests of app interfaces. \
Does the screenshot jevision-screenshot show what is expected? When the reference picture \
jevision-reference is given, it shows how the screen looked when the test was made: judge \
whether the screenshot means the same — the same screen, state and content — and ignore \
differences in exact pixels, fonts, colours, positions, times and dates unless the expectation \
mentions them. Flat boxes of one colour hide private parts of the screen on purpose; ignore \
them. Give the probability that the answer is yes.";
/// For JEV finding an element on a screenshot by its description.
const JEV_LOCATE_INSTRUCTIONS: &str = "You find elements of app interfaces on screenshots for \
automated tests. Flat boxes of one colour hide private parts of the screen on purpose; the \
element is never one of them.";
const JEV_SCREENSHOT: &str = "jevision-screenshot";
const JEV_REFERENCE: &str = "jevision-reference";
/// Sides JEV accepts, in pixels; it turns down pictures outside them instead of scaling.
const JEV_MIN_SIDE: u32 = 280;
const JEV_MAX_SIDE: u32 = 1568;

/// Who answers the questions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Gemini,
    /// TypeSafe AI's image-understanding model.
    Jev,
    Ollama,
    LmStudio,
    /// Any server with the OpenAI Chat Completions API, at an address of the user's choice.
    OpenAiCompatible,
}

impl Provider {
    pub const ALL: [Self; 7] = [
        Self::Anthropic,
        Self::OpenAi,
        Self::Gemini,
        Self::Jev,
        Self::Ollama,
        Self::LmStudio,
        Self::OpenAiCompatible,
    ];

    /// Stable identifier for settings and the command line.
    pub fn id(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::Jev => "jev",
            Self::Ollama => "ollama",
            Self::LmStudio => "lmstudio",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Google Gemini",
            Self::Jev => "JEV (TypeSafe AI)",
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::OpenAiCompatible => "OpenAI-compatible server",
        }
    }

    /// Where the provider's API is when the settings leave the address empty.
    pub fn default_address(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::Jev => "https://api.typesafe.ai",
            Self::Ollama => "http://localhost:11434",
            Self::LmStudio => "http://localhost:1234/v1",
            Self::OpenAiCompatible => "",
        }
    }

    /// A model that sees pictures, for providers whose models cannot be listed.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-5-5",
            Self::OpenAi => "gpt-5-mini",
            Self::Gemini => "gemini-2.5-flash",
            Self::Jev => "jev-latest",
            Self::Ollama | Self::LmStudio | Self::OpenAiCompatible => "",
        }
    }

    /// Name of the API key: the environment variable, and the Keychain item the app saves.
    pub fn key_name(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::OpenAi => Some("OPENAI_API_KEY"),
            Self::Gemini => Some("GEMINI_API_KEY"),
            Self::Jev => Some("TYPESAFE_API_KEY"),
            Self::OpenAiCompatible => Some("OPENAI_COMPATIBLE_API_KEY"),
            Self::Ollama | Self::LmStudio => None,
        }
    }

    /// Questions fail without a key.
    pub fn needs_key(self) -> bool {
        matches!(
            self,
            Self::Anthropic | Self::OpenAi | Self::Gemini | Self::Jev
        )
    }

    /// The user may point it at another address than [`default_address`](Self::default_address).
    pub fn address_editable(self) -> bool {
        !self.needs_key() || self == Self::Jev
    }

    /// [`Client::models`] can ask the provider which models it has.
    pub fn lists_models(self) -> bool {
        matches!(self, Self::Ollama | Self::LmStudio | Self::OpenAiCompatible)
    }

    /// The chosen model may be one that reads only text, so the user says whether it sees.
    pub fn may_lack_pictures(self) -> bool {
        matches!(
            self,
            Self::Jev | Self::Ollama | Self::LmStudio | Self::OpenAiCompatible
        )
    }
}

/// Which provider and model to ask.
#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub provider: Provider,
    pub model: String,
    /// Base address of the API; empty for the provider's default.
    pub address: String,
    pub key: Option<String>,
    /// The model understands pictures; without it, questions stop before anything is sent.
    pub pictures: bool,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("address", &self.address)
            .field("key", &self.key.as_ref().map(|_| MASK))
            .field("pictures", &self.pictures)
            .finish()
    }
}

impl Config {
    /// The base address in use, without a trailing slash.
    pub fn address(&self) -> &str {
        let address = self.address.trim();
        let address = if address.is_empty() {
            self.provider.default_address()
        } else {
            address
        };
        address.trim_end_matches('/')
    }

    /// Pictures stay on this computer: the address points to this machine. Cloud providers
    /// never count as local.
    pub fn is_local(&self) -> bool {
        let rest = self
            .address()
            .split_once("://")
            .map_or(self.address(), |(_, rest)| rest);
        let authority = rest.split('/').next().unwrap_or_default();
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = match host.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default(),
            None => host.split(':').next().unwrap_or_default(),
        };
        host.eq_ignore_ascii_case("localhost")
            || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    fn key(&self) -> Option<&str> {
        self.key.as_deref().map(str::trim).filter(|k| !k.is_empty())
    }
}

/// What to look at and what to look for. Pictures are PNG, private parts already covered.
#[derive(Debug, Clone, Copy)]
pub struct Question<'a> {
    /// What the screen should show, in plain words; empty to only compare with `reference`.
    pub expect: &'a str,
    pub screenshot: &'a MaskedImage,
    pub reference: Option<&'a MaskedImage>,
    /// Model to ask instead of the configured one; empty for the configured one.
    pub model: &'a str,
}

/// A model's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    pub passed: bool,
    /// Why, in a sentence.
    pub reason: String,
    pub usage: Usage,
}

/// Tokens a question used, as the provider counted them; zero when it did not say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Where an element described in words is on a screenshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Located {
    /// In fractions (0–1) of the screenshot, from the top-left corner.
    pub area: Rect,
    /// How sure the model is of the box, from 0 to 1.
    pub confidence: f64,
}

/// What a search for an element described in words came back with, and what it cost, whether
/// or not the element was there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Search {
    /// `None` when the element is more likely not on the screenshot.
    pub found: Option<Located>,
    pub usage: Usage,
}

/// Why a question got no verdict. [`code`](Self::code) and [`params`](Self::params) name a
/// localizable text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisionError {
    NoPictures,
    NoModel,
    NoKey,
    BadAddress {
        address: String,
    },
    Unreachable {
        address: String,
    },
    Timeout {
        seconds: u64,
    },
    BadKey,
    UnknownModel {
        model: String,
    },
    RateLimited {
        message: String,
    },
    Service {
        status: u16,
        message: String,
    },
    Refused {
        message: String,
    },
    BadAnswer {
        answer: String,
    },
    /// The provider can't find elements by description; only JEV can.
    CannotLocate,
    /// The JEV model is older than the first one that finds elements.
    ModelCannotLocate,
}

impl VisionError {
    /// The provider is down or busy for now, rather than set up wrong: another may answer.
    pub fn unavailable(&self) -> bool {
        match self {
            Self::Unreachable { .. } | Self::Timeout { .. } | Self::RateLimited { .. } => true,
            Self::Service { status, .. } => *status >= 500,
            _ => false,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::NoPictures => "run.vision.noPictures",
            Self::NoModel => "run.vision.noModel",
            Self::NoKey => "run.vision.noKey",
            Self::BadAddress { .. } => "run.vision.badAddress",
            Self::Unreachable { .. } => "run.vision.unreachable",
            Self::Timeout { .. } => "run.vision.timeout",
            Self::BadKey => "run.vision.badKey",
            Self::UnknownModel { .. } => "run.vision.unknownModel",
            Self::RateLimited { .. } => "run.vision.rateLimited",
            Self::Service { .. } => "run.vision.serviceError",
            Self::Refused { .. } => "run.vision.refused",
            Self::BadAnswer { .. } => "run.vision.badAnswer",
            Self::CannotLocate => "run.vision.cannotLocate",
            Self::ModelCannotLocate => "run.vision.modelCannotLocate",
        }
    }

    pub fn params(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::NoPictures
            | Self::NoModel
            | Self::NoKey
            | Self::BadKey
            | Self::CannotLocate
            | Self::ModelCannotLocate => Vec::new(),
            Self::BadAddress { address } | Self::Unreachable { address } => {
                vec![("address", address.clone())]
            }
            Self::Timeout { seconds } => vec![("seconds", seconds.to_string())],
            Self::UnknownModel { model } => vec![("model", model.clone())],
            Self::RateLimited { message } | Self::Refused { message } => {
                vec![("message", message.clone())]
            }
            Self::Service { status, message } => {
                vec![("status", status.to_string()), ("message", message.clone())]
            }
            Self::BadAnswer { answer } => vec![("answer", answer.clone())],
        }
    }
}

impl fmt::Display for VisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        for (name, value) in self.params() {
            write!(f, " {name}={value}")?;
        }
        Ok(())
    }
}

impl std::error::Error for VisionError {}

/// Judges screenshots. [`Client`] is the real one; tests use their own.
pub trait VisionJudge: Send + Sync {
    /// Provider name for reports, such as `Anthropic`.
    fn provider(&self) -> &str;
    /// Stable provider identifier, such as `anthropic` ([`Provider::id`]).
    fn provider_id(&self) -> &str;
    fn model(&self) -> &str;
    /// Pictures stay on this computer.
    fn is_local(&self) -> bool;
    fn judge(&self, question: &Question<'_>) -> Result<Judgement, VisionError>;
    /// Finds the element `what` describes on `screenshot`.
    fn locate(&self, _screenshot: &MaskedImage, _what: &str) -> Result<Search, VisionError> {
        Err(VisionError::CannotLocate)
    }
}

/// Talks to one provider.
pub struct Client {
    config: Config,
    timeout: Duration,
    agent: ureq::Agent,
}

impl Client {
    /// A client whose requests give up after `timeout`; [`TIMEOUT`] suits most uses.
    pub fn new(config: Config, timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT.min(timeout)))
            .timeout_global(Some(timeout))
            .user_agent(concat!("JEVision/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            config,
            timeout,
            agent,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Asks about a small built-in picture, to check address, key and model in one go.
    pub fn test(&self) -> Result<Judgement, VisionError> {
        let picture = MaskedImage::from_png(TEST_PICTURE, &[])
            .expect("a picture with nothing to cover passes unchanged");
        self.judge(&Question {
            expect: TEST_EXPECTATION,
            screenshot: &picture,
            reference: None,
            model: "",
        })
    }

    /// Models the provider has, sorted; for Ollama only those that see pictures. Empty for
    /// providers that cannot list them ([`Provider::lists_models`]).
    pub fn models(&self) -> Result<Vec<String>, VisionError> {
        let c = &self.config;
        let mut models = match c.provider {
            Provider::Ollama => {
                let tags = self.get_json(&format!("{}/api/tags", c.address()))?;
                let names = tags["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m["name"].as_str().or(m["model"].as_str()));
                let mut seeing = Vec::new();
                for name in names {
                    if self.ollama_sees_pictures(name)? {
                        seeing.push(name.to_owned());
                    }
                }
                seeing
            }
            Provider::LmStudio | Provider::OpenAiCompatible => {
                let list = self.get_json(&format!("{}/models", c.address()))?;
                list["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m["id"].as_str().map(str::to_owned))
                    .collect()
            }
            Provider::Anthropic | Provider::OpenAi | Provider::Gemini | Provider::Jev => Vec::new(),
        };
        models.sort();
        models.dedup();
        Ok(models)
    }

    /// Ollama names what a model can do; older versions do not, so their models stay listed.
    fn ollama_sees_pictures(&self, model: &str) -> Result<bool, VisionError> {
        let url = format!("{}/api/show", self.config.address());
        let (status, body) = self.send(&url, Some(&json!({ "model": model })))?;
        if status != 200 {
            return Ok(true);
        }
        let shown: Value = serde_json::from_str(&body).unwrap_or_default();
        Ok(match shown["capabilities"].as_array() {
            Some(caps) => caps.iter().any(|c| c == "vision"),
            None => true,
        })
    }

    fn get_json(&self, url: &str) -> Result<Value, VisionError> {
        let (status, body) = self.send(url, None)?;
        match status {
            200 => {}
            // A list that is not there means a wrong address, not a missing model.
            404 => {
                return Err(VisionError::Service {
                    status,
                    message: self.excerpt(&server_message(&body)),
                });
            }
            _ => return Err(self.http_error(status, &body)),
        }
        serde_json::from_str(&body).map_err(|_| VisionError::BadAnswer {
            answer: self.excerpt(&body),
        })
    }

    /// POSTs `body` as JSON, or GETs without one. Any HTTP status is returned, not an error.
    fn send(&self, url: &str, body: Option<&Value>) -> Result<(u16, String), VisionError> {
        let address = self.config.address();
        if !(address.starts_with("http://") || address.starts_with("https://")) {
            return Err(VisionError::BadAddress {
                address: address.to_owned(),
            });
        }
        let headers = headers(&self.config);
        let result = match body {
            Some(body) => {
                let mut request = self.agent.post(url);
                for (name, value) in &headers {
                    request = request.header(*name, value);
                }
                request
                    .content_type("application/json")
                    .send(body.to_string())
            }
            None => {
                let mut request = self.agent.get(url);
                for (name, value) in &headers {
                    request = request.header(*name, value);
                }
                request.call()
            }
        };
        let mut response = result.map_err(|e| self.transport_error(e))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| self.transport_error(e))?;
        Ok((status, body))
    }

    fn transport_error(&self, e: ureq::Error) -> VisionError {
        let address = self.config.address().to_owned();
        match e {
            ureq::Error::Timeout(_) => VisionError::Timeout {
                seconds: self.timeout.as_secs().max(1),
            },
            ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                VisionError::Timeout {
                    seconds: self.timeout.as_secs().max(1),
                }
            }
            ureq::Error::BadUri(_) | ureq::Error::Http(_) => VisionError::BadAddress { address },
            _ => VisionError::Unreachable { address },
        }
    }

    fn http_error(&self, status: u16, body: &str) -> VisionError {
        let message = self.excerpt(&server_message(body));
        match status {
            401 => VisionError::BadKey,
            400 if body.contains("API_KEY_INVALID") => VisionError::BadKey,
            404 => VisionError::UnknownModel {
                model: self.config.model.trim().to_owned(),
            },
            429 => VisionError::RateLimited { message },
            _ => VisionError::Service { status, message },
        }
    }

    /// Settings without which no question is sent.
    fn ready(&self) -> Result<(), VisionError> {
        let c = &self.config;
        if !c.pictures {
            return Err(VisionError::NoPictures);
        }
        if c.model.trim().is_empty() {
            return Err(VisionError::NoModel);
        }
        if c.provider.needs_key() && c.key().is_none() {
            return Err(VisionError::NoKey);
        }
        Ok(())
    }

    /// Up to [`EXCERPT`] characters of `text` on one line, the key masked.
    fn excerpt(&self, text: &str) -> String {
        let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(key) = self.config.key() {
            text = text.replace(key, MASK);
        }
        match text.char_indices().nth(EXCERPT) {
            Some((end, _)) => format!("{}…", &text[..end]),
            None => text,
        }
    }
}

impl VisionJudge for Client {
    fn provider(&self) -> &str {
        self.config.provider.name()
    }

    fn provider_id(&self) -> &str {
        self.config.provider.id()
    }

    fn model(&self) -> &str {
        self.config.model.trim()
    }

    fn is_local(&self) -> bool {
        self.config.is_local()
    }

    fn judge(&self, question: &Question<'_>) -> Result<Judgement, VisionError> {
        let model = question.model.trim();
        if !model.is_empty() && model != self.config.model.trim() {
            let client = Self {
                config: Config {
                    model: model.to_owned(),
                    ..self.config.clone()
                },
                timeout: self.timeout,
                agent: self.agent.clone(),
            };
            return client.judge(&Question {
                model: "",
                ..*question
            });
        }
        let c = &self.config;
        self.ready()?;
        let (url, body) = request(c, question);
        let (status, text) = self.send(&url, Some(&body))?;
        if status != 200 {
            return Err(self.http_error(status, &text));
        }
        let mut judgement = answer(c.provider, &text).map_err(|e| match e {
            VisionError::BadAnswer { answer } => VisionError::BadAnswer {
                answer: self.excerpt(&answer),
            },
            VisionError::Refused { message } => VisionError::Refused {
                message: self.excerpt(&message),
            },
            other => other,
        })?;
        judgement.reason = self.excerpt(&judgement.reason);
        Ok(judgement)
    }

    fn locate(&self, screenshot: &MaskedImage, what: &str) -> Result<Search, VisionError> {
        let c = &self.config;
        if c.provider != Provider::Jev {
            return Err(VisionError::CannotLocate);
        }
        self.ready()?;
        if !jev_locates(c.model.trim()) {
            return Err(VisionError::ModelCannotLocate);
        }
        let (url, body, fill) = locate_request(c, screenshot, what);
        let (status, text) = self.send(&url, Some(&body))?;
        if status != 200 {
            return Err(self.http_error(status, &text));
        }
        located(&text, fill).map_err(|e| match e {
            VisionError::BadAnswer { answer } => VisionError::BadAnswer {
                answer: self.excerpt(&answer),
            },
            other => other,
        })
    }
}

/// Request headers, the key among them.
fn headers(c: &Config) -> Vec<(&'static str, String)> {
    let key = c.key();
    match c.provider {
        Provider::Anthropic => {
            let mut h = vec![("anthropic-version", "2023-06-01".to_owned())];
            h.extend(key.map(|k| ("x-api-key", k.to_owned())));
            h
        }
        Provider::Gemini => key
            .map(|k| ("x-goog-api-key", k.to_owned()))
            .into_iter()
            .collect(),
        Provider::Jev => {
            // Without it the API turns down requests with pictures.
            let mut h = vec![("typesafe-danger-zone-images", "true".to_owned())];
            h.extend(key.map(|k| ("authorization", format!("Bearer {k}"))));
            h
        }
        Provider::OpenAi | Provider::LmStudio | Provider::OpenAiCompatible | Provider::Ollama => {
            key.map(|k| ("authorization", format!("Bearer {k}")))
                .into_iter()
                .collect()
        }
    }
}

/// The verdict's shape as JSON Schema.
fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "passed": {
                "type": "boolean",
                "description": "The screenshot shows what is expected."
            },
            "reason": {
                "type": "string",
                "description": "One short sentence on what decides it."
            }
        },
        "required": ["passed", "reason"],
        "additionalProperties": false
    })
}

fn expectation(q: &Question<'_>) -> String {
    let expect = q.expect.trim();
    if expect.is_empty() {
        "Expected: the screenshot means the same as the reference picture.".to_owned()
    } else {
        format!("Expected: {expect}")
    }
}

/// URL and JSON body that ask `q`.
fn request(c: &Config, q: &Question<'_>) -> (String, Value) {
    let b64 = |png: &[u8]| base64::engine::general_purpose::STANDARD.encode(png);
    let model = c.model.trim();
    let base = c.address();
    let pictures: Vec<(&str, String)> = std::iter::once(("Screenshot:", b64(q.screenshot.png())))
        .chain(q.reference.map(|r| ("Reference picture:", b64(r.png()))))
        .collect();
    let expected = expectation(q);
    match c.provider {
        Provider::Anthropic => {
            let mut content = Vec::new();
            for (label, data) in &pictures {
                content.push(json!({ "type": "text", "text": label }));
                content.push(json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": "image/png", "data": data }
                }));
            }
            content.push(json!({ "type": "text", "text": expected }));
            let body = json!({
                "model": model,
                "max_tokens": 1024,
                "temperature": 0,
                "system": INSTRUCTIONS,
                "tools": [{
                    "name": "verdict",
                    "description": "Report whether the screenshot shows what is expected.",
                    "input_schema": schema()
                }],
                "tool_choice": { "type": "tool", "name": "verdict" },
                "messages": [{ "role": "user", "content": content }]
            });
            (format!("{base}/v1/messages"), body)
        }
        Provider::Gemini => {
            let mut parts = Vec::new();
            for (label, data) in &pictures {
                parts.push(json!({ "text": label }));
                parts.push(json!({ "inlineData": { "mimeType": "image/png", "data": data } }));
            }
            parts.push(json!({ "text": expected }));
            let body = json!({
                "systemInstruction": { "parts": [{ "text": INSTRUCTIONS }] },
                "contents": [{ "role": "user", "parts": parts }],
                "generationConfig": {
                    "temperature": 0,
                    "responseMimeType": "application/json",
                    "responseSchema": {
                        "type": "OBJECT",
                        "properties": {
                            "passed": { "type": "BOOLEAN" },
                            "reason": { "type": "STRING" }
                        },
                        "required": ["passed", "reason"],
                        "propertyOrdering": ["passed", "reason"]
                    }
                }
            });
            let model = model.strip_prefix("models/").unwrap_or(model);
            (
                format!("{base}/v1beta/models/{model}:generateContent"),
                body,
            )
        }
        Provider::Ollama => {
            let labels: Vec<&str> = pictures.iter().map(|(label, _)| *label).collect();
            let images: Vec<&String> = pictures.iter().map(|(_, data)| data).collect();
            let body = json!({
                "model": model,
                "stream": false,
                "format": schema(),
                "options": { "temperature": 0 },
                "messages": [
                    { "role": "system", "content": INSTRUCTIONS },
                    {
                        "role": "user",
                        "content": format!("Pictures in order: {}\n{expected}", labels.join(" ")),
                        "images": images
                    }
                ]
            });
            (format!("{base}/api/chat"), body)
        }
        Provider::OpenAi | Provider::LmStudio | Provider::OpenAiCompatible => {
            let mut content = Vec::new();
            for (label, data) in &pictures {
                content.push(json!({ "type": "text", "text": label }));
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:image/png;base64,{data}") }
                }));
            }
            content.push(json!({ "type": "text", "text": expected }));
            let mut body = json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": INSTRUCTIONS },
                    { "role": "user", "content": content }
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": { "name": "verdict", "strict": true, "schema": schema() }
                }
            });
            // Reasoning models at OpenAI take only the default temperature.
            if c.provider != Provider::OpenAi {
                body["temperature"] = json!(0);
            }
            (format!("{base}/chat/completions"), body)
        }
        Provider::Jev => {
            let asset = |png: &[u8]| jev_asset(&jev_sized(png));
            let mut state = json!({ "screenshot": JEV_SCREENSHOT });
            let mut assets = json!({ JEV_SCREENSHOT: asset(q.screenshot.png()) });
            if let Some(reference) = q.reference {
                state["reference"] = json!(JEV_REFERENCE);
                assets[JEV_REFERENCE] = asset(reference.png());
            }
            let body = json!({
                "model": model,
                "state": state,
                "assets": assets,
                "questions": {
                    "passed": { "type": "noul", "instructions": format!("{JEV_INSTRUCTIONS}\n{expected}") }
                }
            });
            (format!("{base}/v1/systemone"), body)
        }
    }
}

/// URL and JSON body that ask JEV where the element `what` describes is, and whether it is
/// there at all: a box always has four edges, even around nothing. Also the share of the
/// width and height of the sent picture that the screenshot fills.
fn locate_request(c: &Config, screenshot: &MaskedImage, what: &str) -> (String, Value, (f64, f64)) {
    let (png, fill) = jev_fitted(screenshot.png());
    let what = what.trim();
    let body = json!({
        "model": c.model.trim(),
        "state": { "screenshot": JEV_SCREENSHOT },
        "assets": { JEV_SCREENSHOT: jev_asset(&png) },
        "questions": {
            "visible": {
                "type": "noul",
                "instructions": format!("{JEV_LOCATE_INSTRUCTIONS}\nIs this element visible in the screenshot {JEV_SCREENSHOT}: {what}? Give the probability that the answer is yes.")
            },
            "where": {
                "type": "box",
                "instructions": format!("{JEV_LOCATE_INSTRUCTIONS}\nThe element in the screenshot {JEV_SCREENSHOT}: {what}")
            }
        }
    });
    (format!("{}/v1/systemone", c.address()), body, fill)
}

/// JEV models before 1.14 can't answer where something is; names without a version, such as
/// `jev-latest`, are taken to be new enough.
fn jev_locates(model: &str) -> bool {
    let version = model.strip_prefix("jev-").unwrap_or(model);
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= (1, 14),
        _ => true,
    }
}

fn jev_asset(png: &[u8]) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    json!({ "type": "image", "format": "png", "data_base64": data })
}

/// `png` with every side within what JEV accepts: scaled to fit, and where the shape is too
/// long for that, padded with [`MASK_COLOUR`] on the right or bottom. Bytes that are not a PNG
/// pass as they are.
fn jev_sized(png: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    jev_fitted(png).0
}

/// [`jev_sized`], with the share of the width and height that the picture fills; the rest is
/// padding.
fn jev_fitted(png: &[u8]) -> (std::borrow::Cow<'_, [u8]>, (f64, f64)) {
    let whole = (1.0, 1.0);
    let Ok(picture) = image::Image::decode(png) else {
        return (png.into(), whole);
    };
    let long = f64::from(picture.width.max(picture.height));
    let short = f64::from(picture.width.min(picture.height));
    let scale = if long > f64::from(JEV_MAX_SIDE) {
        f64::from(JEV_MAX_SIDE) / long
    } else if short < f64::from(JEV_MIN_SIDE) {
        (f64::from(JEV_MIN_SIDE) / short).min(f64::from(JEV_MAX_SIDE) / long)
    } else {
        return (png.into(), whole);
    };
    let side = |s: u32| ((f64::from(s) * scale).round() as u32).max(1);
    let fitted = picture.resized(side(picture.width), side(picture.height));
    let width = fitted.width.max(JEV_MIN_SIDE);
    let height = fitted.height.max(JEV_MIN_SIDE);
    let mut rgba = MASK_COLOUR.repeat((width * height) as usize);
    let row = fitted.width as usize * 4;
    for (y, line) in fitted.rgba.chunks_exact(row).enumerate() {
        let start = y * width as usize * 4;
        rgba[start..start + row].copy_from_slice(line);
    }
    let sized = image::Image {
        width,
        height,
        rgba,
    };
    let fill = (
        f64::from(fitted.width) / f64::from(width),
        f64::from(fitted.height) / f64::from(height),
    );
    match sized.encode() {
        Ok(bytes) => (bytes.into(), fill),
        Err(_) => (png.into(), whole),
    }
}

/// Reads where JEV saw the element from a successful response body, and the tokens it used.
/// `fill` is the share of the sent picture the screenshot fills.
fn located(body: &str, (fill_x, fill_y): (f64, f64)) -> Result<Search, VisionError> {
    let bad = || VisionError::BadAnswer {
        answer: body.to_owned(),
    };
    let v: Value = serde_json::from_str(body).map_err(|_| bad())?;
    let usage = usage(Provider::Jev, &v);
    let found = |found| Ok(Search { found, usage });
    let fraction = |v: &Value| v.as_f64().filter(|p| (0.0..=1.0).contains(p));
    let visible = fraction(&v["answers"]["visible"]["noul"]).ok_or_else(bad)?;
    if visible < 0.5 {
        return found(None);
    }
    let answer = &v["answers"]["where"];
    let edge = |name: &str| fraction(&answer["box"][name]).ok_or_else(bad);
    let (top, bottom, left, right) = (edge("top")?, edge("bottom")?, edge("left")?, edge("right")?);
    if left >= right || top >= bottom {
        return Err(bad());
    }
    // Edges count from the whole sent picture, padding included.
    let x = |e: f64| (e / fill_x).min(1.0);
    let y = |e: f64| (e / fill_y).min(1.0);
    let area = Rect {
        x: x(left),
        y: y(top),
        width: x(right) - x(left),
        height: y(bottom) - y(top),
    };
    if area.width <= 0.0 || area.height <= 0.0 {
        // The box lies on the padding, off the screenshot.
        return found(None);
    }
    found(Some(Located {
        area,
        confidence: fraction(&answer["confidence"]).unwrap_or(visible),
    }))
}

/// Reads the verdict from a successful response body.
fn answer(provider: Provider, body: &str) -> Result<Judgement, VisionError> {
    let bad = |text: &str| VisionError::BadAnswer {
        answer: text.to_owned(),
    };
    let v: Value = serde_json::from_str(body).map_err(|_| bad(body))?;
    let mut judgement = verdict_of(provider, &v)?;
    judgement.usage = usage(provider, &v);
    Ok(judgement)
}

/// The tokens a successful response says it used.
pub(crate) fn usage(provider: Provider, v: &Value) -> Usage {
    let count = |v: &Value| v.as_u64().unwrap_or_default();
    let (input, output) = match provider {
        Provider::Anthropic | Provider::Jev => {
            let u = &v["usage"];
            (
                count(&u["input_tokens"])
                    + count(&u["cache_creation_input_tokens"])
                    + count(&u["cache_read_input_tokens"]),
                count(&u["output_tokens"]),
            )
        }
        Provider::Gemini => {
            let u = &v["usageMetadata"];
            (
                count(&u["promptTokenCount"]),
                count(&u["candidatesTokenCount"]) + count(&u["thoughtsTokenCount"]),
            )
        }
        Provider::Ollama => (count(&v["prompt_eval_count"]), count(&v["eval_count"])),
        Provider::OpenAi | Provider::LmStudio | Provider::OpenAiCompatible => {
            let u = &v["usage"];
            (count(&u["prompt_tokens"]), count(&u["completion_tokens"]))
        }
    };
    Usage {
        input_tokens: input,
        output_tokens: output,
    }
}

/// The verdict in a successful response, without its usage.
fn verdict_of(provider: Provider, v: &Value) -> Result<Judgement, VisionError> {
    let bad = |text: &str| VisionError::BadAnswer {
        answer: text.to_owned(),
    };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    match provider {
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
            match content.iter().find(|c| c["type"] == "tool_use") {
                Some(tool) => {
                    verdict(&tool["input"]).ok_or_else(|| bad(&tool["input"].to_string()))
                }
                None => verdict_in(&said),
            }
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
            match candidate["finishReason"].as_str() {
                Some(
                    r @ ("SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII"),
                ) if said.trim().is_empty() => Err(VisionError::Refused {
                    message: r.to_owned(),
                }),
                _ => verdict_in(&said),
            }
        }
        Provider::Ollama => verdict_in(&text(&v["message"]["content"])),
        Provider::OpenAi | Provider::LmStudio | Provider::OpenAiCompatible => {
            let message = &v["choices"][0]["message"];
            if let Some(refusal) = message["refusal"].as_str() {
                return Err(VisionError::Refused {
                    message: refusal.to_owned(),
                });
            }
            verdict_in(&text(&message["content"]))
        }
        Provider::Jev => {
            let yes = v["answers"]["passed"]["noul"]
                .as_f64()
                .filter(|p| (0.0..=1.0).contains(p))
                .ok_or_else(|| bad(&v.to_string()))?;
            let passed = yes >= 0.5;
            let sure = ((if passed { yes } else { 1.0 - yes }) * 100.0).round() as i64;
            let reason = if passed {
                jevision_l10n::tr!("vision.jev.passed", sure)
            } else {
                jevision_l10n::tr!("vision.jev.failed", sure)
            };
            Ok(Judgement {
                passed,
                reason,
                usage: Usage::default(),
            })
        }
    }
}

/// The verdict in a model's text: plain JSON, or JSON wrapped in prose or a code fence.
fn verdict_in(said: &str) -> Result<Judgement, VisionError> {
    let found = serde_json::from_str::<Value>(said.trim()).ok().or_else(|| {
        let start = said.find('{')?;
        let end = said.rfind('}')?;
        serde_json::from_str(said.get(start..=end)?).ok()
    });
    found
        .as_ref()
        .and_then(verdict)
        .ok_or_else(|| VisionError::BadAnswer {
            answer: said.to_owned(),
        })
}

fn verdict(v: &Value) -> Option<Judgement> {
    let passed = match &v["passed"] {
        Value::Bool(b) => *b,
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "pass" | "passed" => true,
            "false" | "no" | "fail" | "failed" => false,
            _ => return None,
        },
        _ => return None,
    };
    let reason = v["reason"].as_str().unwrap_or_default().trim().to_owned();
    Some(Judgement {
        passed,
        reason,
        usage: Usage::default(),
    })
}

/// The human part of an error response from any of the providers.
fn server_message(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return body.to_owned();
    };
    [
        &v["error"]["message"],
        &v["error"],
        &v["message"],
        &v["detail"],
    ]
    .into_iter()
    .find_map(|m| m.as_str())
    .map_or_else(|| body.to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests;
