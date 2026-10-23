//! Vision models for checks by meaning: the providers, the models a server offers, a quick
//! test that the settings work, and what the calls cost. Calls go over the network and block; call them off the main
//! thread.

use std::fmt;
use std::sync::{Arc, OnceLock};
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
    /// The provider can find the elements tests describe in words.
    pub finds_elements: bool,
    /// Environment variable that can hold the account the requests go to; empty when the
    /// provider needs none.
    pub account_variable: String,
}

/// Which model to ask and how to reach it.
#[derive(Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct VisionSettings {
    /// A [`VisionProvider::id`].
    pub provider: String,
    pub model: String,
    /// Empty for the provider's default address.
    pub address: String,
    pub key: Option<String>,
    /// The model understands pictures; checks that send screenshots stop when it doesn't.
    pub pictures: bool,
    /// The account requests go to, for providers with a [`VisionProvider::account_variable`].
    #[uniffi(default = "")]
    pub account_id: String,
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
            .field("account_id", &self.account_id)
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
                account_id: self.account_id.clone(),
            },
            TIMEOUT,
        ))
    }
}

/// The settings of each provider, asked for only when a check by meaning names that provider
/// or a block needs it, so a run reads no API key it doesn't need.
#[uniffi::export(with_foreign)]
pub trait ProviderSettings: Send + Sync {
    /// `None` when the provider isn't set up.
    fn settings(&self, provider: String) -> Option<VisionSettings>;
}

impl fmt::Debug for dyn ProviderSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ProviderSettings")
    }
}

/// One provider's judge, which asks for its settings the first time a check needs more than
/// its id.
pub(crate) struct NamedJudge {
    provider: Provider,
    source: Arc<dyn ProviderSettings>,
    client: OnceLock<VisionClient>,
}

impl NamedJudge {
    pub(crate) fn new(provider: Provider, source: &Arc<dyn ProviderSettings>) -> Self {
        Self {
            provider,
            source: source.clone(),
            client: OnceLock::new(),
        }
    }

    pub(crate) fn every(source: &Arc<dyn ProviderSettings>) -> Vec<Self> {
        Provider::ALL
            .into_iter()
            .map(|provider| Self::new(provider, source))
            .collect()
    }

    fn client(&self) -> &VisionClient {
        self.client.get_or_init(|| {
            let s = self
                .source
                .settings(self.provider.id().to_owned())
                // Not set up: no key, so a provider that needs one says so.
                .unwrap_or_else(|| VisionSettings {
                    model: self.provider.default_model().to_owned(),
                    pictures: true,
                    ..VisionSettings::default()
                });
            VisionClient::new(
                VisionConfig {
                    provider: self.provider,
                    model: s.model,
                    address: s.address,
                    key: s.key,
                    pictures: s.pictures,
                    account_id: s.account_id,
                },
                TIMEOUT,
            )
        })
    }
}

impl VisionJudge for NamedJudge {
    fn provider(&self) -> &str {
        self.provider.name()
    }
    fn provider_id(&self) -> &str {
        self.provider.id()
    }
    fn model(&self) -> &str {
        self.client().model()
    }
    fn is_local(&self) -> bool {
        self.client().is_local()
    }
    fn judge(
        &self,
        question: &jevision_runner::Question<'_>,
    ) -> Result<jevision_runner::Judgement, jevision_runner::VisionError> {
        self.client().judge(question)
    }
    fn locate(
        &self,
        screenshot: &jevision_runner::MaskedImage,
        what: &str,
    ) -> Result<jevision_runner::Search, jevision_runner::VisionError> {
        self.client().locate(screenshot, what)
    }
}

/// Why a vision model could not be asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum VisionCallError {
    #[error("{}", jevision_l10n::tr!("vision.error.unknownProvider", .provider))]
    UnknownProvider { provider: String },
    /// A `run.vision.*` code with its values, as in run reports but without a block, and the
    /// The sentence for it in the current language.
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
            finds_elements: p.is_jev(),
            account_variable: p.account_name().unwrap_or_default().into(),
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
    #[error("{}", jevision_l10n::tr!("vision.error.usageUnreadable", .reason))]
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
pub(crate) mod tests {
    use super::*;

    /// Settings that note every provider asked for.
    #[derive(Default)]
    pub(crate) struct Asked(pub(crate) std::sync::Mutex<Vec<String>>);

    impl ProviderSettings for Asked {
        fn settings(&self, provider: String) -> Option<VisionSettings> {
            self.0.lock().unwrap().push(provider.clone());
            (provider == "ollama").then(|| VisionSettings {
                provider,
                model: "llava".into(),
                ..VisionSettings::default()
            })
        }
    }

    #[test]
    fn a_run_reads_only_the_settings_of_the_providers_its_checks_name() {
        let asked = Arc::new(Asked::default());
        let source: Arc<dyn ProviderSettings> = asked.clone();
        let judges = NamedJudge::every(&source);
        let ids: Vec<&str> = judges.iter().map(|j| j.provider_id()).collect();
        assert_eq!(ids, jevision_core::AI_PROVIDERS);
        assert!(asked.0.lock().unwrap().is_empty());

        let ollama = judges.iter().find(|j| j.provider_id() == "ollama").unwrap();
        assert_eq!(ollama.provider(), "Ollama");
        assert!(asked.0.lock().unwrap().is_empty());
        assert_eq!(ollama.model(), "llava");
        assert!(ollama.is_local());
        assert_eq!(*asked.0.lock().unwrap(), ["ollama"]);

        // A provider that isn't set up keeps its usual model and has no key, so its checks say
        // the key is missing.
        let openai = judges.iter().find(|j| j.provider_id() == "openai").unwrap();
        assert!(!openai.is_local());
        assert_eq!(openai.model(), Provider::OpenAi.default_model());
        assert_eq!(*asked.0.lock().unwrap(), ["ollama", "openai"]);
    }

    #[test]
    fn a_named_judge_reads_its_settings_only_when_it_is_used() {
        let asked = Arc::new(Asked::default());
        let source: Arc<dyn ProviderSettings> = asked.clone();
        let jev = NamedJudge::new(Provider::Jev, &source);
        assert_eq!(jev.provider(), "JEV (TypeSafe AI)");
        assert!(asked.0.lock().unwrap().is_empty());
        assert!(!jev.is_local());
        assert_eq!(jev.model(), Provider::Jev.default_model());
        // JEV isn't set up: no key, so finding stops before anything is sent.
        let picture = jevision_runner::MaskedImage::from_png(b"\x89PNG", &[]).unwrap();
        assert_eq!(
            jev.locate(&picture, "the Save button"),
            Err(jevision_runner::VisionError::NoKey)
        );
        assert_eq!(*asked.0.lock().unwrap(), ["jev"]);
    }

    fn settings(provider: &str, address: &str, key: Option<&str>) -> VisionSettings {
        VisionSettings {
            provider: provider.into(),
            model: "m".into(),
            address: address.into(),
            key: key.map(Into::into),
            pictures: true,
            account_id: String::new(),
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
                "jev-cloudflare",
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
        assert_eq!(
            unsure,
            [
                "jev",
                "jev-cloudflare",
                "ollama",
                "lmstudio",
                "openai-compatible"
            ]
        );
        let finders: Vec<_> = vision_providers()
            .into_iter()
            .filter(|p| p.finds_elements)
            .map(|p| (p.id, p.account_variable))
            .collect();
        assert_eq!(
            finders,
            [
                ("jev".to_owned(), String::new()),
                (
                    "jev-cloudflare".to_owned(),
                    "CLOUDFLARE_ACCOUNT_ID".to_owned()
                )
            ]
        );
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
            "“ftp://localhost” isn’t a web address. It should start with http:// or https://."
        );
        assert_eq!(
            vision_test(settings("nope", "", None)),
            Err(VisionCallError::UnknownProvider {
                provider: "nope".into()
            })
        );
    }
}
