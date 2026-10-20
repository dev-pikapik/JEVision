//! What AI calls cost: a journal of the tokens every call used, a price table that turns them
//! into money, and daily and monthly limits per provider.
//!
//! The journal is a JSON Lines file on this computer, one [`UsageRecord`] per line, shared by
//! the app and the command line. Costs are worked out when the journal is read, from the price
//! table in use, so a corrected price applies to past calls too. Days and months are those of
//! this computer's time zone.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::Usage;
use crate::report::Failure;
use crate::run::rfc3339;

/// Share of a limit after which AI steps warn that it is close.
pub const WARN_SHARE: f64 = 0.8;

/// One AI call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    /// When the answer came, RFC 3339 in UTC.
    pub time: String,
    /// Provider identifier, such as `anthropic`.
    pub provider: String,
    pub model: String,
    /// Test file, relative to the project folder.
    pub test: String,
    pub block: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What a provider's models cost, in US dollars per million tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Price {
    /// Provider identifier, such as `openai`.
    pub provider: String,
    /// Start of the model names the price is for, such as `gpt-5-mini`; empty for every model of
    /// the provider. The longest match wins.
    pub model: String,
    pub input: f64,
    pub output: f64,
}

impl Price {
    /// What `input` and `output` tokens cost at this price.
    pub fn cost(&self, input: u64, output: u64) -> f64 {
        (input as f64 * self.input + output as f64 * self.output) / 1_000_000.0
    }
}

/// The price of `model` at `provider`: the row with the longest matching model name.
pub fn price_of<'a>(prices: &'a [Price], provider: &str, model: &str) -> Option<&'a Price> {
    prices
        .iter()
        .filter(|p| p.provider == provider && model.starts_with(p.model.trim()))
        .max_by_key(|p| p.model.trim().len())
}

/// Public list prices of the models people pick most, as known when this version was made.
/// Providers change them; the user can correct them in the settings.
pub fn default_prices() -> Vec<Price> {
    let row = |provider: &str, model: &str, input, output| Price {
        provider: provider.into(),
        model: model.into(),
        input,
        output,
    };
    vec![
        row("anthropic", "claude-opus", 5.0, 25.0),
        row("anthropic", "claude-sonnet", 3.0, 15.0),
        row("anthropic", "claude-haiku", 1.0, 5.0),
        row("openai", "gpt-5", 1.25, 10.0),
        row("openai", "gpt-5-mini", 0.25, 2.0),
        row("openai", "gpt-5-nano", 0.05, 0.4),
        row("openai", "gpt-4.1", 2.0, 8.0),
        row("openai", "gpt-4.1-mini", 0.4, 1.6),
        row("openai", "gpt-4o", 2.5, 10.0),
        row("openai", "gpt-4o-mini", 0.15, 0.6),
        row("gemini", "gemini-2.5-pro", 1.25, 10.0),
        row("gemini", "gemini-2.5-flash", 0.3, 2.5),
        row("gemini", "gemini-2.5-flash-lite", 0.1, 0.4),
        row("ollama", "", 0.0, 0.0),
        row("lmstudio", "", 0.0, 0.0),
    ]
}

/// How much a provider may cost, in US dollars; `None` for no limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub provider: String,
    pub daily: Option<f64>,
    pub monthly: Option<f64>,
}

/// Money and tokens over a period.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spend {
    /// US dollars, for the calls whose model has a price.
    pub cost: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub calls: u64,
    /// Calls to models without a price; their cost is not in [`cost`](Self::cost).
    pub unpriced_calls: u64,
}

impl Spend {
    fn add(&mut self, r: &UsageRecord, price: Option<&Price>) {
        self.input_tokens += r.input_tokens;
        self.output_tokens += r.output_tokens;
        self.calls += 1;
        match price {
            Some(p) => self.cost += p.cost(r.input_tokens, r.output_tokens),
            None => self.unpriced_calls += 1,
        }
    }
}

/// A provider's spend today and this month.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSpend {
    pub provider: String,
    pub today: Spend,
    pub month: Spend,
}

/// What one provider cost on one day.
#[derive(Debug, Clone, PartialEq)]
pub struct DaySpend {
    /// `YYYY-MM-DD`, in this computer's time zone.
    pub day: String,
    pub provider: String,
    pub spend: Spend,
}

/// Spend per provider, and per day over recent days.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageSummary {
    /// Providers used this month, by identifier.
    pub providers: Vec<ProviderSpend>,
    /// Days with calls, oldest first; each provider on its own row.
    pub days: Vec<DaySpend>,
}

/// Where a provider stands against its limits.
#[derive(Debug, Clone, PartialEq)]
pub enum LimitState {
    Within,
    /// Past [`WARN_SHARE`] of a limit: `run.vision.limitNear`.
    Near(Failure),
    /// A limit is used up: `run.vision.limitReached`.
    Reached(Failure),
}

/// The journal, the prices and the limits.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageBook {
    /// The journal file; made on the first call.
    pub journal: PathBuf,
    pub prices: Vec<Price>,
    pub limits: Vec<Limit>,
}

impl UsageBook {
    /// Adds a call that just got its answer, with the tokens it used.
    pub fn log(
        &self,
        provider: &str,
        model: &str,
        test: &str,
        block: &str,
        usage: Usage,
    ) -> io::Result<()> {
        self.record(&UsageRecord {
            time: rfc3339(SystemTime::now()),
            provider: provider.to_owned(),
            model: model.to_owned(),
            test: test.to_owned(),
            block: block.to_owned(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        })
    }

    /// The failure for a journal that can't be read or written, so spending can't be kept to
    /// its limits.
    pub fn journal_failure(&self, e: &io::Error) -> Failure {
        Failure::new("run.vision.journalFailed")
            .with("path", self.journal.display())
            .with("reason", e.to_string())
    }

    /// Adds a call to the journal.
    pub fn record(&self, record: &UsageRecord) -> io::Result<()> {
        if let Some(folder) = self.journal.parent() {
            fs::create_dir_all(folder)?;
        }
        let mut line = serde_json::to_string(record).map_err(io::Error::other)?;
        line.push('\n');
        // One write per line, appended, so the app and the command line can log at once.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.journal)?
            .write_all(line.as_bytes())
    }

    /// Every call in the journal; none when there is no journal yet. Lines that can't be read
    /// (a write cut short) are skipped.
    pub fn records(&self) -> io::Result<Vec<UsageRecord>> {
        let text = match fs::read_to_string(&self.journal) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(text
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    /// Spend per provider today and this month, and per day over the `days` days up to today.
    pub fn summary(&self, now: SystemTime, days: u32) -> io::Result<UsageSummary> {
        let today = local_day(now);
        let first = today - chrono::Days::new(u64::from(days.saturating_sub(1)));
        let mut summary = UsageSummary::default();
        for r in self.records()? {
            let Some(day) = record_day(&r) else { continue };
            let price = price_of(&self.prices, &r.provider, &r.model);
            if same_month(day, today) && day <= today {
                let at = match summary
                    .providers
                    .iter()
                    .position(|p| p.provider == r.provider)
                {
                    Some(at) => at,
                    None => {
                        summary.providers.push(ProviderSpend {
                            provider: r.provider.clone(),
                            today: Spend::default(),
                            month: Spend::default(),
                        });
                        summary.providers.len() - 1
                    }
                };
                let p = &mut summary.providers[at];
                p.month.add(&r, price);
                if day == today {
                    p.today.add(&r, price);
                }
            }
            if day >= first && day <= today {
                let name = day.format("%Y-%m-%d").to_string();
                match summary
                    .days
                    .iter_mut()
                    .find(|d| d.day == name && d.provider == r.provider)
                {
                    Some(d) => d.spend.add(&r, price),
                    None => {
                        let mut spend = Spend::default();
                        spend.add(&r, price);
                        summary.days.push(DaySpend {
                            day: name,
                            provider: r.provider.clone(),
                            spend,
                        });
                    }
                }
            }
        }
        summary
            .days
            .sort_by(|a, b| (&a.day, &a.provider).cmp(&(&b.day, &b.provider)));
        summary
            .providers
            .sort_by(|a, b| a.provider.cmp(&b.provider));
        Ok(summary)
    }

    /// Where `provider` stands against its limits at `now`. `name` is how the provider is
    /// called in the failure.
    pub fn limit_state(
        &self,
        provider: &str,
        name: &str,
        now: SystemTime,
    ) -> io::Result<LimitState> {
        let Some(limit) = self.limits.iter().find(|l| l.provider == provider) else {
            return Ok(LimitState::Within);
        };
        if limit.daily.is_none() && limit.monthly.is_none() {
            return Ok(LimitState::Within);
        }
        let summary = self.summary(now, 1)?;
        let spent = summary.providers.iter().find(|p| p.provider == provider);
        let (today, month) = spent.map_or((0.0, 0.0), |p| (p.today.cost, p.month.cost));
        let periods = [("day", limit.daily, today), ("month", limit.monthly, month)];
        let failure = |code: &str, period: &str, limit: f64, spent: f64| {
            Failure::new(code)
                .with("provider", name)
                .with("period", period)
                .with("limit", money(limit))
                .with("spent", money(spent))
        };
        for (period, limit, spent) in periods {
            if let Some(limit) = limit.filter(|&l| spent >= l) {
                return Ok(LimitState::Reached(failure(
                    "run.vision.limitReached",
                    period,
                    limit,
                    spent,
                )));
            }
        }
        for (period, limit, spent) in periods {
            if let Some(limit) = limit.filter(|&l| spent >= l * WARN_SHARE) {
                return Ok(LimitState::Near(failure(
                    "run.vision.limitNear",
                    period,
                    limit,
                    spent,
                )));
            }
        }
        Ok(LimitState::Within)
    }
}

/// Dollars as the journal's failures carry them: a plain number with cents, such as `12.40`.
pub fn money(dollars: f64) -> String {
    format!("{dollars:.2}")
}

fn local_day(time: SystemTime) -> NaiveDate {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    DateTime::from_timestamp(since.as_secs() as i64, 0)
        .unwrap_or_default()
        .with_timezone(&Local)
        .date_naive()
}

fn record_day(r: &UsageRecord) -> Option<NaiveDate> {
    let time = DateTime::parse_from_rfc3339(&r.time).ok()?;
    Some(time.with_timezone(&Local).date_naive())
}

fn same_month(a: NaiveDate, b: NaiveDate) -> bool {
    a.year() == b.year() && a.month() == b.month()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn book(name: &str) -> UsageBook {
        let journal = std::env::temp_dir()
            .join(format!("jevision-usage-{name}-{}", std::process::id()))
            .join("usage.jsonl");
        let _ = fs::remove_file(&journal);
        UsageBook {
            journal,
            prices: default_prices(),
            limits: Vec::new(),
        }
    }

    fn call(time: SystemTime, provider: &str, model: &str, input: u64, output: u64) -> UsageRecord {
        UsageRecord {
            time: crate::run::rfc3339(time),
            provider: provider.into(),
            model: model.into(),
            test: "tests/cart.json".into(),
            block: "check".into(),
            input_tokens: input,
            output_tokens: output,
        }
    }

    /// Noon on the 15th of a month in this computer's time zone: days around it stay in it.
    fn mid_month() -> SystemTime {
        use chrono::TimeZone;
        let noon = Local.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
        UNIX_EPOCH + Duration::from_secs(noon.timestamp() as u64)
    }

    const DAY: Duration = Duration::from_secs(86_400);

    #[test]
    fn the_longest_matching_price_wins() {
        let prices = default_prices();
        let model = |provider, model| price_of(&prices, provider, model).map(|p| p.model.as_str());
        assert_eq!(model("openai", "gpt-5-mini-2025-08-07"), Some("gpt-5-mini"));
        assert_eq!(model("openai", "gpt-5"), Some("gpt-5"));
        assert_eq!(
            model("anthropic", "claude-sonnet-5-5"),
            Some("claude-sonnet")
        );
        assert_eq!(model("ollama", "qwen2.5vl:7b"), Some(""));
        assert_eq!(model("openai", "o3"), None);
        assert_eq!(model("openai-compatible", "gpt-5"), None);
        let sonnet = price_of(&prices, "anthropic", "claude-sonnet-5-5").unwrap();
        assert!((sonnet.cost(1_000_000, 100_000) - 4.5).abs() < 1e-9);
    }

    #[test]
    fn spend_is_summed_per_provider_day_and_month() {
        let b = book("summary");
        let now = mid_month();
        for r in [
            call(now, "anthropic", "claude-sonnet-5-5", 1_000_000, 0),
            call(now - DAY, "anthropic", "claude-haiku-4-5", 0, 1_000_000),
            call(
                now - DAY * 40,
                "anthropic",
                "claude-sonnet-5-5",
                1_000_000,
                0,
            ),
            call(now, "openai-compatible", "mystery", 10, 5),
            call(now, "ollama", "qwen2.5vl:7b", 800, 20),
        ] {
            b.record(&r).unwrap();
        }
        fs::OpenOptions::new()
            .append(true)
            .open(&b.journal)
            .unwrap()
            .write_all(b"{\"time\": cut sh")
            .unwrap();

        let s = b.summary(now, 7).unwrap();
        let ids: Vec<&str> = s.providers.iter().map(|p| p.provider.as_str()).collect();
        assert_eq!(ids, ["anthropic", "ollama", "openai-compatible"]);
        let anthropic = &s.providers[0];
        assert!((anthropic.today.cost - 3.0).abs() < 1e-9);
        assert!((anthropic.month.cost - 8.0).abs() < 1e-9);
        assert_eq!(anthropic.month.calls, 2);
        assert_eq!(s.providers[1].month.cost, 0.0);
        assert_eq!(s.providers[2].today.unpriced_calls, 1);
        assert_eq!(s.providers[2].today.input_tokens, 10);

        let days: Vec<(&str, &str)> = s
            .days
            .iter()
            .map(|d| (&d.day[8..], d.provider.as_str()))
            .collect();
        assert_eq!(
            days,
            [
                ("14", "anthropic"),
                ("15", "anthropic"),
                ("15", "ollama"),
                ("15", "openai-compatible")
            ]
        );
        assert_eq!(b.summary(now, 1).unwrap().days.len(), 3);
        assert_eq!(
            book("empty").summary(now, 30).unwrap(),
            UsageSummary::default()
        );
    }

    #[test]
    fn limits_warn_first_then_stop() {
        let mut b = book("limits");
        let now = mid_month();
        let state = |b: &UsageBook| b.limit_state("anthropic", "Anthropic", now).unwrap();
        assert_eq!(state(&b), LimitState::Within);
        b.limits = vec![Limit {
            provider: "anthropic".into(),
            daily: Some(5.0),
            monthly: Some(20.0),
        }];
        // $3.75 of $5 today is under 80 %.
        b.record(&call(now, "anthropic", "claude-sonnet", 1_250_000, 0))
            .unwrap();
        assert_eq!(state(&b), LimitState::Within);
        // $4.50 is past it.
        b.record(&call(now, "anthropic", "claude-sonnet", 250_000, 0))
            .unwrap();
        let LimitState::Near(near) = state(&b) else {
            panic!("expected a warning");
        };
        assert_eq!(near.code, "run.vision.limitNear");
        assert_eq!(near.param("period"), "day");
        assert_eq!(near.param("spent"), "4.50");
        assert_eq!(near.param("limit"), "5.00");
        assert_eq!(near.param("provider"), "Anthropic");

        b.record(&call(now, "anthropic", "claude-sonnet", 0, 40_000))
            .unwrap();
        let LimitState::Reached(reached) = state(&b) else {
            panic!("expected the limit to be reached");
        };
        assert_eq!(reached.code, "run.vision.limitReached");
        assert_eq!(reached.param("period"), "day");
        // Another provider has no limit.
        assert_eq!(
            b.limit_state("openai", "OpenAI", now).unwrap(),
            LimitState::Within
        );
        // Tomorrow the day starts over, the month does not.
        b.limits[0].monthly = Some(5.5);
        let LimitState::Near(month) = b.limit_state("anthropic", "Anthropic", now + DAY).unwrap()
        else {
            panic!("expected a warning for the month");
        };
        assert_eq!(month.param("period"), "month");
    }
}
