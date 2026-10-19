use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

use super::*;

const KEY: &str = "sk-test-0123456789abcdef";
const SHOT: &[u8] = b"\x89PNG screenshot";
const REFERENCE: &[u8] = b"\x89PNG reference";
static SHOT_PICTURE: LazyLock<MaskedImage> =
    LazyLock::new(|| MaskedImage::from_png(SHOT, &[]).unwrap());
static REFERENCE_PICTURE: LazyLock<MaskedImage> =
    LazyLock::new(|| MaskedImage::from_png(REFERENCE, &[]).unwrap());

fn config(provider: Provider, address: &str) -> Config {
    Config {
        provider,
        model: "vision-model".into(),
        address: address.into(),
        key: provider.key_name().map(|_| KEY.to_owned()),
        pictures: true,
    }
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn question() -> Question<'static> {
    Question {
        expect: "The cart shows 2 items",
        screenshot: &SHOT_PICTURE,
        reference: Some(&REFERENCE_PICTURE),
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Images in a request body, decoded, in order.
fn images(body: &Value) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    fn walk(v: &Value, found: &mut Vec<Vec<u8>>) {
        let decode = |s: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(s.trim_start_matches("data:image/png;base64,"))
                .ok()
        };
        match v {
            Value::Object(map) => {
                for (k, v) in map {
                    match (k.as_str(), v) {
                        ("data" | "url" | "data_base64", Value::String(s)) => {
                            found.extend(decode(s))
                        }
                        ("images", Value::Array(list)) => {
                            found.extend(list.iter().filter_map(|i| decode(i.as_str()?)))
                        }
                        _ => walk(v, found),
                    }
                }
            }
            Value::Array(list) => list.iter().for_each(|v| walk(v, found)),
            _ => {}
        }
    }
    walk(body, &mut found);
    found
}

#[test]
fn requests_carry_both_pictures_the_expectation_and_a_strict_answer_shape() {
    for provider in Provider::ALL {
        let c = config(provider, "http://localhost:9");
        let (url, body) = request(&c, &question());
        // JEV names its pictures instead of ordering them.
        let sent = match provider {
            Provider::Jev => [JEV_SCREENSHOT, JEV_REFERENCE]
                .iter()
                .flat_map(|name| images(&body["assets"][name]))
                .collect(),
            _ => images(&body),
        };
        assert_eq!(sent, [SHOT, REFERENCE], "{provider:?}");
        let text = body.to_string();
        assert!(text.contains("The cart shows 2 items"), "{provider:?}");
        assert!(!text.contains(KEY), "{provider:?} puts the key in the body");
        if provider == Provider::Jev {
            assert_eq!(body["questions"]["passed"]["type"], "noul");
        } else {
            assert!(text.contains("\"passed\"") && text.contains("\"reason\""));
        }
        let expected_url = match provider {
            Provider::Anthropic => "http://localhost:9/v1/messages",
            Provider::Jev => "http://localhost:9/v1/systemone",
            Provider::Gemini => "http://localhost:9/v1beta/models/vision-model:generateContent",
            Provider::Ollama => "http://localhost:9/api/chat",
            _ => "http://localhost:9/chat/completions",
        };
        assert_eq!(url, expected_url);
    }

    let c = config(Provider::Anthropic, "");
    let (url, body) = request(&c, &question());
    assert_eq!(url, "https://api.anthropic.com/v1/messages");
    assert_eq!(body["tool_choice"]["name"], "verdict");
    assert_eq!(body["tools"][0]["input_schema"], schema());

    let (_, body) = request(&config(Provider::OpenAi, ""), &question());
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert!(body.get("temperature").is_none());
    let (_, body) = request(&config(Provider::LmStudio, ""), &question());
    assert_eq!(body["temperature"], 0);

    let (_, body) = request(&config(Provider::Gemini, ""), &question());
    assert_eq!(
        body["generationConfig"]["responseMimeType"],
        "application/json"
    );
    let (_, body) = request(&config(Provider::Ollama, ""), &question());
    assert_eq!(body["format"], schema());
    assert_eq!(body["stream"], false);

    // JEV turns down pictures that the state or the instructions do not name.
    let (url, body) = request(&config(Provider::Jev, ""), &question());
    assert_eq!(url, "https://api.typesafe.ai/v1/systemone");
    let instructions = body["questions"]["passed"]["instructions"]
        .as_str()
        .unwrap();
    for name in body["assets"].as_object().unwrap().keys() {
        assert!(body["state"].to_string().contains(name) && instructions.contains(name));
    }

    let only_shot = Question {
        expect: "",
        screenshot: &SHOT_PICTURE,
        reference: None,
    };
    let (_, body) = request(&config(Provider::Ollama, ""), &only_shot);
    assert_eq!(images(&body), [SHOT]);
    assert!(body.to_string().contains("means the same as the reference"));
    let (_, body) = request(&config(Provider::Jev, ""), &only_shot);
    assert_eq!(images(&body), [SHOT]);
    assert!(body["state"].get("reference").is_none());
}

#[test]
fn pictures_for_jev_get_sides_it_accepts() {
    let png = |width, height| {
        let rgba = [200, 30, 30, 255].repeat((width * height) as usize);
        image::Image {
            width,
            height,
            rgba,
        }
        .encode()
        .unwrap()
    };
    let sides = |width, height| {
        let sized = image::Image::decode(&jev_sized(&png(width, height))).unwrap();
        (sized.width, sized.height, sized)
    };
    // Small pictures grow as a whole, so "the left half" still means the same.
    let (width, height, small) = sides(100, 50);
    assert_eq!((width, height), (560, 280));
    assert_eq!(&small.rgba[small.rgba.len() - 4..], [200, 30, 30, 255]);
    let (width, height, long) = sides(3000, 100);
    assert_eq!((width, height), (1568, 280));
    assert_eq!(&long.rgba[..4], [200, 30, 30, 255]);
    assert_eq!(&long.rgba[long.rgba.len() - 4..], MASK_COLOUR);
    let (width, height, _) = sides(1000, 50);
    assert_eq!((width, height), (1568, 280));
    let (width, height, _) = sides(4000, 3000);
    assert_eq!((width, height), (1568, 1176));
    let fine = png(800, 600);
    assert_eq!(*jev_sized(&fine), fine[..]);
    assert_eq!(*jev_sized(SHOT), *SHOT);
}

#[test]
fn keys_go_in_headers_only() {
    let h = |p| headers(&config(p, ""));
    assert!(h(Provider::Anthropic).contains(&("x-api-key", KEY.to_owned())));
    assert!(h(Provider::Gemini).contains(&("x-goog-api-key", KEY.to_owned())));
    assert!(h(Provider::OpenAi).contains(&("authorization", format!("Bearer {KEY}"))));
    assert!(h(Provider::Jev).contains(&("authorization", format!("Bearer {KEY}"))));
    assert!(h(Provider::Jev).contains(&("typesafe-danger-zone-images", "true".to_owned())));
    assert!(h(Provider::Ollama).is_empty());
    let no_key = Config {
        key: None,
        ..config(Provider::OpenAiCompatible, "")
    };
    assert!(headers(&no_key).is_empty());
    let printed = format!("{:?}", config(Provider::OpenAi, ""));
    assert!(!printed.contains(KEY) && printed.contains(MASK));
}

#[test]
fn answers_of_every_provider_are_read() {
    let read = |p, name| answer(p, &fixture(name)).unwrap();
    assert_eq!(
        read(Provider::Anthropic, "anthropic.json"),
        Judgement {
            passed: true,
            reason: "The cart badge shows 2 items.".into()
        }
    );
    assert_eq!(
        read(Provider::OpenAi, "openai.json"),
        Judgement {
            passed: false,
            reason: "The cart is empty.".into()
        }
    );
    assert_eq!(
        read(Provider::Gemini, "gemini.json").reason,
        "Корзина показывает 2 товара."
    );
    assert!(!read(Provider::Ollama, "ollama.json").passed);
    // Prose around a fenced block and "yes" for true, as small local models answer.
    assert_eq!(
        read(Provider::LmStudio, "lmstudio.json"),
        Judgement {
            passed: true,
            reason: "The Save button is visible.".into()
        }
    );
    assert_eq!(
        read(Provider::Jev, "jev.json"),
        Judgement {
            passed: false,
            reason: "JEV is 96% sure the screenshot does not meet the expectation.".into()
        }
    );
}

#[test]
fn refusals_and_unreadable_answers_are_errors() {
    let err = |p, body: &str| answer(p, body).unwrap_err();
    assert!(matches!(
        err(Provider::Anthropic, &fixture("anthropic-refusal.json")),
        VisionError::Refused { .. }
    ));
    assert_eq!(
        err(Provider::OpenAi, &fixture("openai-refusal.json")),
        VisionError::Refused {
            message: "I'm sorry, I can't assist with that request.".into()
        }
    );
    assert_eq!(
        err(Provider::Gemini, &fixture("gemini-blocked.json")),
        VisionError::Refused {
            message: "SAFETY".into()
        }
    );
    let rambling = r#"{"message": {"content": "The cart looks fine to me."}}"#;
    assert_eq!(
        err(Provider::Ollama, rambling),
        VisionError::BadAnswer {
            answer: "The cart looks fine to me.".into()
        }
    );
    let maybe = r#"{"message": {"content": "{\"passed\": \"maybe\", \"reason\": \"\"}"}}"#;
    assert!(matches!(
        err(Provider::Ollama, maybe),
        VisionError::BadAnswer { .. }
    ));
    assert!(matches!(
        err(Provider::OpenAi, "<html>"),
        VisionError::BadAnswer { .. }
    ));
    let no_probability = r#"{"answers": {"passed": {"type": "noul"}}}"#;
    assert!(matches!(
        err(Provider::Jev, no_probability),
        VisionError::BadAnswer { .. }
    ));
}

#[test]
fn only_this_computer_counts_as_local() {
    let local = |p, a: &str| config(p, a).is_local();
    assert!(local(Provider::Ollama, ""));
    assert!(local(Provider::LmStudio, ""));
    assert!(local(
        Provider::OpenAiCompatible,
        "http://127.0.0.1:8080/v1"
    ));
    assert!(local(Provider::OpenAiCompatible, "http://[::1]:8080/v1"));
    assert!(local(Provider::Ollama, "http://LOCALHOST:11434/"));
    assert!(!local(Provider::Ollama, "http://192.168.1.20:11434"));
    assert!(!local(
        Provider::OpenAiCompatible,
        "http://localhost.example.com/v1"
    ));
    assert!(!local(
        Provider::OpenAiCompatible,
        "http://localhost@example.com/v1"
    ));
    assert!(!local(Provider::OpenAiCompatible, ""));
    for cloud in [
        Provider::Anthropic,
        Provider::OpenAi,
        Provider::Gemini,
        Provider::Jev,
    ] {
        assert!(!local(cloud, ""));
    }
}

/// A one-request-per-connection HTTP server on this computer that answers with `replies` in
/// order and keeps every request it got.
struct Server {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

fn serve(replies: Vec<(u16, String)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    thread::spawn(move || {
        for (status, body) in replies {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let mut content = vec![0; length];
            reader.read_exact(&mut content).unwrap();
            head.push_str(&String::from_utf8_lossy(&content));
            seen.lock().unwrap().push(head);
            let mut stream = reader.into_inner();
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Server { address, requests }
}

fn client(provider: Provider, address: &str) -> Client {
    Client::new(config(provider, address), Duration::from_secs(5))
}

#[test]
fn judges_over_http() {
    let server = serve(vec![(200, fixture("anthropic.json"))]);
    let judgement = client(Provider::Anthropic, &server.address)
        .judge(&question())
        .unwrap();
    assert!(judgement.passed);
    let sent = server.requests.lock().unwrap()[0].clone();
    assert!(sent.starts_with("POST /v1/messages "));
    assert!(
        sent.to_ascii_lowercase()
            .contains(&format!("x-api-key: {KEY}"))
    );
    assert!(sent.contains(&b64(SHOT)));
}

#[test]
fn http_failures_become_clear_errors_without_the_key() {
    let echo = format!(r#"{{"error": {{"message": "Something broke for key {KEY}."}}}}"#);
    let server = serve(vec![
        (
            401,
            r#"{"error": {"message": "Incorrect API key provided"}}"#.into(),
        ),
        (404, r#"{"error": "model 'vision-model' not found"}"#.into()),
        (429, r#"{"error": {"message": "Quota exceeded"}}"#.into()),
        (500, echo),
        (
            200,
            format!(r#"{{"choices": [{{"message": {{"content": "{KEY}"}}}}]}}"#),
        ),
    ]);
    let c = client(Provider::OpenAiCompatible, &server.address);
    let mut errors = Vec::new();
    for _ in 0..5 {
        errors.push(c.judge(&question()).unwrap_err());
    }
    assert_eq!(errors[0], VisionError::BadKey);
    assert_eq!(
        errors[1],
        VisionError::UnknownModel {
            model: "vision-model".into()
        }
    );
    assert_eq!(
        errors[2],
        VisionError::RateLimited {
            message: "Quota exceeded".into()
        }
    );
    assert_eq!(
        errors[3],
        VisionError::Service {
            status: 500,
            message: format!("Something broke for key {MASK}.")
        }
    );
    assert_eq!(
        errors[4],
        VisionError::BadAnswer {
            answer: MASK.into()
        }
    );
    for e in &errors {
        assert!(!format!("{e} {e:?}").contains(KEY), "{e:?}");
    }

    let gemini = serve(vec![(
        400,
        r#"{"error": {"code": 400, "message": "API key not valid.", "status": "INVALID_ARGUMENT", "details": [{"reason": "API_KEY_INVALID"}]}}"#.into(),
    )]);
    assert_eq!(
        client(Provider::Gemini, &gemini.address)
            .judge(&question())
            .unwrap_err(),
        VisionError::BadKey
    );
}

#[test]
fn missing_settings_fail_before_anything_is_sent() {
    let mut c = config(Provider::Anthropic, "http://127.0.0.1:9");
    c.key = Some("  ".into());
    let judge = |c: &Config| {
        Client::new(c.clone(), Duration::from_secs(1))
            .judge(&question())
            .unwrap_err()
    };
    assert_eq!(judge(&c), VisionError::NoKey);
    c.model = " ".into();
    assert_eq!(judge(&c), VisionError::NoModel);
    // A text-only model: the question stops before the closed port is tried.
    let text_only = Config {
        pictures: false,
        ..config(Provider::Ollama, "http://localhost:9")
    };
    assert_eq!(judge(&text_only), VisionError::NoPictures);
    let test = Client::new(text_only, Duration::from_secs(1)).test();
    assert_eq!(test.unwrap_err(), VisionError::NoPictures);
    let c = config(Provider::OpenAiCompatible, "localhost:1234/v1");
    assert_eq!(
        judge(&c),
        VisionError::BadAddress {
            address: "localhost:1234/v1".into()
        }
    );
}

#[test]
fn silent_and_absent_servers_time_out_or_are_unreachable() {
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", silent.local_addr().unwrap());
    let c = Client::new(
        config(Provider::Ollama, &address),
        Duration::from_millis(500),
    );
    assert_eq!(
        c.judge(&question()).unwrap_err(),
        VisionError::Timeout { seconds: 1 }
    );
    drop(silent);
    assert_eq!(
        client(Provider::Ollama, &address)
            .judge(&question())
            .unwrap_err(),
        VisionError::Unreachable { address }
    );
}

#[test]
fn local_providers_list_models_that_see() {
    let server = serve(vec![
        (200, fixture("ollama-tags.json")),
        (200, r#"{"capabilities": ["completion", "tools"]}"#.into()),
        (200, r#"{"capabilities": ["completion", "vision"]}"#.into()),
    ]);
    assert_eq!(
        client(Provider::Ollama, &server.address).models().unwrap(),
        ["qwen2.5vl:7b"]
    );
    let requests = server.requests.lock().unwrap();
    assert!(requests[0].starts_with("GET /api/tags "));
    assert!(requests[1].starts_with("POST /api/show ") && requests[1].contains("llama3.2:3b"));

    let server = serve(vec![(200, fixture("lmstudio-models.json"))]);
    let address = format!("{}/v1", server.address);
    assert_eq!(
        client(Provider::LmStudio, &address).models().unwrap(),
        [
            "google/gemma-3-12b",
            "qwen2.5-vl-7b-instruct",
            "text-embedding-nomic-embed-text-v1.5"
        ]
    );
    assert!(server.requests.lock().unwrap()[0].starts_with("GET /v1/models "));
}
