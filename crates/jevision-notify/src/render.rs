//! A message laid out for Slack (Block Kit) and for mail (HTML).

use std::fmt::Write as _;

use jevision_l10n::tr_in;
use serde_json::{Value, json};

use crate::message::{Message, Tone, clip};

/// Id the screenshot goes by inside the mail, as `cid:` in the HTML.
pub(crate) const PICTURE_ID: &str = "failed-step@jevision";

/// Slack's limits: a header holds 150 characters, a section's text 3000, a field 2000, and a
/// section has at most 10 fields.
const HEADER: usize = 150;
const TEXT: usize = 3000;
const FIELD: usize = 2000;
const FIELDS: usize = 10;

/// Slack reads &, < and > as markup.
pub(crate) fn slack_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The body of a Slack webhook post: blocks for the heading, the facts, the failures and the
/// report, and the plain text that notifications and screen readers use.
pub(crate) fn slack(message: &Message) -> Value {
    let (d, lang) = (&message.details, message.lang.as_str());
    let section = |text: String| json!({"type": "section", "text": {"type": "mrkdwn", "text": clip(&text, TEXT)}});
    let mut blocks = vec![json!({
        "type": "header",
        "text": {"type": "plain_text", "text": clip(&message.title, HEADER)},
    })];
    if let Some(lead) = &d.lead {
        blocks.push(section(slack_escape(lead)));
    }
    if !d.facts.is_empty() {
        let fields: Vec<Value> = d
            .facts
            .iter()
            .take(FIELDS)
            .map(|(label, value)| {
                let text = format!("*{}*\n{}", slack_escape(label), slack_escape(value));
                json!({"type": "mrkdwn", "text": clip(&text, FIELD)})
            })
            .collect();
        blocks.push(json!({"type": "section", "fields": fields}));
    }
    if !d.failures.is_empty() {
        let mut lines: Vec<String> = d
            .failures
            .iter()
            .map(|f| match f.reason.as_str() {
                "" => format!("• *{}*", slack_escape(&f.test)),
                reason => format!("• *{}*: {}", slack_escape(&f.test), slack_escape(reason)),
            })
            .collect();
        if d.unlisted > 0 {
            lines.push(tr_in!(lang, "and %lld more", d.unlisted));
        }
        blocks.push(section(lines.join("\n")));
    }
    if !d.fixed.is_empty() {
        blocks.push(section(format!(
            "*{}:* {}",
            slack_escape(&tr_in!(lang, "notify.fixed")),
            slack_escape(&d.fixed.join(", "))
        )));
    }
    if let Some(report) = &d.report {
        let target = slack_escape(&report.target);
        let text = if report.is_link {
            let open = match report.system.as_str() {
                "GitHub Actions" => tr_in!(lang, "notify.report.openOnGitHub"),
                system => tr_in!(lang, "notify.report.openIn", system),
            };
            format!(
                "{}: <{target}|{}>",
                slack_escape(&report.label),
                slack_escape(&open)
            )
        } else {
            format!("{}: `{target}`", slack_escape(&report.label))
        };
        blocks.push(json!({
            "type": "context",
            "elements": [{"type": "mrkdwn", "text": clip(&text, TEXT)}],
        }));
    }
    json!({"text": slack_escape(&message.text()), "blocks": blocks})
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The mail's HTML: a coloured heading, the facts, the failures and, when the message has a
/// screenshot, the picture inline. Styles are inline because mail programs ignore the rest.
pub(crate) fn html(message: &Message) -> String {
    let (d, lang) = (&message.details, message.lang.as_str());
    let colour = match d.tone {
        Tone::Bad => "#b3261e",
        Tone::Good => "#1e7a34",
        Tone::Neutral => "#48484a",
    };
    let muted = "color:#636366";
    let mut page = format!(
        "<!doctype html>\n<html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
         <meta name=\"color-scheme\" content=\"light\"><title>{title}</title></head>\n\
         <body style=\"margin:0;padding:24px;background:#f2f2f7;font-family:-apple-system,Helvetica,Arial,sans-serif;color:#1c1c1e\">\n\
         <div style=\"max-width:620px;margin:0 auto;background:#ffffff;border:1px solid #d1d1d6;border-radius:12px;overflow:hidden\">\n\
         <h1 style=\"margin:0;padding:18px 24px;background:{colour};color:#ffffff;font-size:20px;line-height:1.3\">{title}</h1>\n\
         <div style=\"padding:20px 24px;font-size:15px;line-height:1.5\">\n",
        title = html_escape(&message.title),
        lang = html_escape(lang),
    );
    if let Some(lead) = &d.lead {
        let _ = writeln!(
            page,
            "<p style=\"margin:0 0 16px\">{}</p>",
            html_escape(lead)
        );
    }
    if !d.failures.is_empty() {
        page.push_str("<ul style=\"margin:0 0 16px;padding-left:20px\">\n");
        for f in &d.failures {
            let _ = write!(
                page,
                "<li style=\"margin:0 0 6px\"><strong>{}</strong>",
                html_escape(&f.test)
            );
            if !f.reason.is_empty() {
                let _ = write!(page, ": {}", html_escape(&f.reason));
            }
            page.push_str("</li>\n");
        }
        if d.unlisted > 0 {
            let _ = writeln!(
                page,
                "<li style=\"list-style:none;{muted}\">{}</li>",
                html_escape(&tr_in!(lang, "and %lld more", d.unlisted))
            );
        }
        page.push_str("</ul>\n");
    }
    if !d.fixed.is_empty() {
        let _ = writeln!(
            page,
            "<p style=\"margin:0 0 16px\"><strong>{}:</strong> {}</p>",
            html_escape(&tr_in!(lang, "notify.fixed")),
            html_escape(&d.fixed.join(", "))
        );
    }
    if !d.facts.is_empty() {
        page.push_str(
            "<table role=\"presentation\" style=\"border-collapse:collapse;margin:0 0 16px\">\n",
        );
        for (label, value) in &d.facts {
            let _ = writeln!(
                page,
                "<tr><td style=\"padding:2px 16px 2px 0;{muted}\">{}</td><td style=\"padding:2px 0\">{}</td></tr>",
                html_escape(label),
                html_escape(value)
            );
        }
        page.push_str("</table>\n");
    }
    if let Some(picture) = &d.picture {
        let _ = writeln!(
            page,
            "<p style=\"margin:0 0 16px\"><img src=\"cid:{PICTURE_ID}\" alt=\"{caption}\" \
             style=\"display:block;max-width:100%;height:auto;border:1px solid #d1d1d6;border-radius:8px\"></p>\n\
             <p style=\"margin:0 0 16px;font-size:13px;{muted}\">{caption}</p>",
            caption = html_escape(&picture.caption)
        );
    }
    if let Some(report) = &d.report {
        let target = html_escape(&report.target);
        let shown = if report.is_link {
            format!("<a href=\"{target}\">{target}</a>")
        } else {
            format!("<code>{target}</code>")
        };
        let _ = writeln!(
            page,
            "<p style=\"margin:0;font-size:13px;{muted};word-break:break-all\">{}: {shown}</p>",
            html_escape(&report.label)
        );
    }
    page.push_str("</div>\n</div>\n</body></html>\n");
    page
}
