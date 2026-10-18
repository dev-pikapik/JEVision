//! Vision models for checks by meaning: the providers, the models a server offers, a quick
//! test that the settings work, and what the calls cost. Calls go over the network and block; call them off the main
//! thread.

use std::fmt;
use std::time::SystemTime;

use jevision_runner::{
    DaySpend, Limit, Price, Provider, ProviderSpend, Spend, TIMEOUT, UsageBook, UsageSummary,
    VisionClient, VisionConfig, VisionJudge,
};

use crate::run::Failure;

/// A service or program that runs vision models.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VisionProvider {
    /// Stable identifier kept in settings, such as `ollama`.
    pub id: String,
    pub name: String,
    /// Where requests go when no address is set; empty when the address must be given.
    pub default_address: String,
    /// Suggested model; empty when the models come from the server.
    pub default_model: String,
    /// Environment variable that can hold the API key; empty when there is no key.
    pub key_variable: String,
    /// Questions fail without an API key.
    pub needs_key: bool,
    /// The server can list its models.
    pub lists_models: bool,
}

/// Which model to ask and how to reach it.
#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct VisionSettings {
    /// A [`VisionProvider::id`].
    pub provider: String,
    pub model: String,
    /// Empty for the provider's default address.
    pub address: String,
    pub key: Option<String>,
}

impl fmt::Debug for VisionSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VisionSettings")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("address", &self.address)
            .field(
                "key",
                &self.key.as_ref().map(|_| jevision_runner::SECRET_MASK),
            )
            .finish()
    }
}

impl VisionSettings {
    pub(crate) fn client(&self) -> Result<VisionClient, VisionCallError> {
        let provider =
            Provider::from_id(&self.provider).ok_or_else(|| VisionCallError::UnknownProvider {
                provider: self.provider.clone(),
            })?;
        Ok(VisionClient::new(
            VisionConfig {
                provider,
                model: self.model.clone(),
                address: self.address.clone(),
                key: self.key.clone(),
            },
            TIMEOUT,
        ))
    }
}

/// Why a vision model could not be asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum VisionCallError {
    #[error("unknown vision provider `{provider}`")]
    UnknownProvider { provider: String },
    /// A `run.vision.*` code with its values, as in run reports but without a block, and the
    /// English sentence for when the app has no translation.
    #[error("{message}")]
    Failed { failure: Failure, message: String },
}

/// The model's answer to the test question.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VisionVerdict {
    /// The model saw the test picture correctly.
    pub passed: bool,
    pub reason: String,
}

#[uniffi::export]
pub fn vision_providers() -> Vec<VisionProvider> {
    Provider::ALL
        .iter()
        .map(|&p| VisionProvider {
            id: p.id().into(),
            name: p.name().into(),
            default_address: p.default_address().into(),
            default_model: p.default_model().into(),
            key_variable: p.key_name().unwrap_or_default().into(),
            needs_key: p.needs_key(),
            lists_models: p.lists_models(),
        })
        .collect()
}

/// Screenshots stay on this Mac: the model runs here, not on a provider's servers.
#[uniffi::export]
pub fn vision_is_local(settings: VisionSettings) -> bool {
    settings.client().is_ok_and(|c| c.is_local())
}

/// Models the server offers, sorted; for Ollama only those that see pictures.
#[uniffi::export]
pub fn vision_models(settings: VisionSettings) -> Result<Vec<String>, VisionCallError> {
    let client = settings.client()?;
    client.models().map_err(|e| failed(&client, &e))
}

/// Asks the model about a small built-in picture: checks address, key and model in one go.
#[uniffi::export]
pub fn vision_test(settings: VisionSettings) -> Result<VisionVerdict, VisionCallError> {
    let client = settings.client()?;
    let judgement = client.test().map_err(|e| failed(&client, &e))?;
    Ok(VisionVerdict {
        passed: judgement.passed,
        reason: judgement.reason,
    })
}

/// Where AI calls are logged, what they cost and how much they may cost.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AiUsageSettings {
    /// The journal file, made on the first call.
    pub journal: String,
    pub prices: Vec<Price>,
    pub limits: Vec<Limit>,
}

impl AiUsageSettings {
    pub(crate) fn book(self) -> UsageBook {
        UsageBook {
            journal: self.journal.into(),
            prices: self.prices,
            limits: self.limits,
        }
    }
}

#[uniffi::remote(Record)]
pub struct Price {
    pub provider: String,
    pub model: String,
    pub input: f64,
    pub output: f64,
}

#[uniffi::remote(Record)]
pub struct Limit {
    pub provider: String,
    pub daily: Option<f64>,
    pub monthly: Option<f64>,
}

#[uniffi::remote(Record)]
pub struct Spend {
    pub cost: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub calls: u64,
    pub unpriced_calls: u64,
}

#[uniffi::remote(Record)]
pub struct ProviderSpend {
    pub provider: String,
    pub today: Spend,
    pub month: Spend,
}

#[uniffi::remote(Record)]
pub struct DaySpend {
    pub day: String,
    pub provider: String,
    pub spend: Spend,
}

#[uniffi::remote(Record)]
pub struct UsageSummary {
    pub providers: Vec<ProviderSpend>,
    pub days: Vec<DaySpend>,
}

/// Public list prices of common models, to start the price table from. In US dollars per
/// million tokens; a row's model is the start of the model names it is for.
#[uniffi::export]
pub fn ai_default_prices() -> Vec<Price> {
    jevision_runner::default_prices()
}

/// Spend per provider today and this month, and per day over the last `days` days, in this
/// Mac's time zone. Fails when the journal can't be read.
#[uniffi::export]
pub fn ai_usage_summary(
    settings: AiUsageSettings,
    days: u32,
) -> Result<UsageSummary, AiUsageError> {
    settings
        .book()
        .summary(SystemTime::now(), days)
        .map_err(|e| AiUsageError::Unreadable {
            reason: e.to_string(),
        })
}

/// Why the usage journal couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum AiUsageError {
    #[error("the AI usage journal can't be read: {reason}")]
    Unreadable { reason: String },
}

fn failed(client: &VisionClient, e: &jevision_runner::VisionError) -> VisionCallError {
    let failure = jevision_runner::vision_failure(client, e);
    VisionCallError::Failed {
        message: jevision_runner::describe_vision(&failure),
        failure: failure.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(provider: &str, address: &str, key: Option<&str>) -> VisionSettings {
        VisionSettings {
            provider: provider.into(),
            model: "m".into(),
            address: address.into(),
            key: key.map(Into::into),
        }
    }

    #[test]
    fn providers_and_locality() {
        let ids: Vec<_> = vision_providers().into_iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            [
                "anthropic",
                "openai",
                "gemini",
                "ollama",
                "lmstudio",
                "openai-compatible"
            ]
        );
        assert!(vision_is_local(settings("ollama", "", None)));
        assert!(vision_is_local(settings(
            "openai-compatible",
            "http://127.0.0.1:8080/v1",
            None
        )));
        assert!(!vision_is_local(settings(
            "openai-compatible",
            "https://models.example.com/v1",
            None
        )));
        assert!(!vision_is_local(settings("openai", "", Some("k"))));
        assert!(!vision_is_local(settings("nope", "", None)));
    }

    #[test]
    fn errors_are_localizable_and_keep_the_key_out() {
        let key = "sk-very-secret-key";
        let s = settings("anthropic", "", Some(key));
        assert!(!format!("{s:?}").contains(key));

        let VisionCallError::Failed { failure, .. } =
            vision_test(settings("anthropic", "", None)).unwrap_err()
        else {
            panic!("expected a failure");
        };
        assert_eq!(failure.code, "run.vision.noKey");
        assert_eq!(failure.params["provider"], "Anthropic");

        let error = vision_models(settings("ollama", "ftp://localhost", None)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "ftp://localhost is not a web address; it should start with http:// or https://."
        );
        assert_eq!(
            vision_test(settings("nope", "", None)),
            Err(VisionCallError::UnknownProvider {
                provider: "nope".into()
            })
        );
    }
}
