//! Vision models for checks by meaning: the providers, the models a server offers and a quick
//! test that the settings work. Calls go over the network and block; call them off the main
//! thread.

use std::fmt;

use jevision_runner::{Provider, TIMEOUT, VisionClient, VisionConfig, VisionJudge};

use crate::run::Failure;

/// A service or program that runs vision models.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VisionProvider {
    /// Stable identifier kept in settings, such as `ollama`.
    pub id: String,
    pub name: String,
    /// Where requests go when no address is set; empty when the address must be given.
    pub default_address: String,
    /// The user may set another address.
    pub address_editable: bool,
    /// Suggested model; empty when the models come from the server.
    pub default_model: String,
    /// Environment variable that can hold the API key; empty when there is no key.
    pub key_variable: String,
    /// Questions fail without an API key.
    pub needs_key: bool,
    /// The server can list its models.
    pub lists_models: bool,
    /// The model may read only text; the user says whether it understands pictures.
    pub may_lack_pictures: bool,
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
    /// The model understands pictures; checks that send screenshots stop when it doesn't.
    pub pictures: bool,
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
            .field("pictures", &self.pictures)
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
                pictures: self.pictures,
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
            address_editable: p.address_editable(),
            default_model: p.default_model().into(),
            key_variable: p.key_name().unwrap_or_default().into(),
            needs_key: p.needs_key(),
            lists_models: p.lists_models(),
            may_lack_pictures: p.may_lack_pictures(),
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
            pictures: true,
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
                "jev",
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
        assert!(!vision_is_local(settings("jev", "", Some("k"))));
        let editable: Vec<_> = vision_providers()
            .into_iter()
            .filter(|p| p.address_editable)
            .map(|p| p.id)
            .collect();
        assert_eq!(editable, ["jev", "ollama", "lmstudio", "openai-compatible"]);
        let unsure: Vec<_> = vision_providers()
            .into_iter()
            .filter(|p| p.may_lack_pictures)
            .map(|p| p.id)
            .collect();
        assert_eq!(unsure, ["jev", "ollama", "lmstudio", "openai-compatible"]);
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
