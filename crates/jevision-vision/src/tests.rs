use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

use super::*;

const KEY: &str = "sk-test-0123456789abcdef";
const ACCOUNT: &str = "0123456789abcdef0123456789abcdef";
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
        account_id: ACCOUNT.into(),
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
        model: "",
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
        let input = match provider {
            Provider::JevCloudflare => &body["input"],
            _ => &body,
        };
        let sent = match provider {
            Provider::Jev | Provider::JevCloudflare => [JEV_SCREENSHOT, JEV_REFERENCE]
                .iter()
                .flat_map(|name| images(&input["assets"][name]))
                .collect(),
            _ => images(&body),
        };
        assert_eq!(sent, [SHOT, REFERENCE], "{provider:?}");
        let text = body.to_string();
        assert!(text.contains("The cart shows 2 items"), "{provider:?}");
        assert!(!text.contains(KEY), "{provider:?} puts the key in the body");
        if provider.is_jev() {
            assert_eq!(input["questions"]["passed"]["type"], "noul");
        } else {
            assert!(text.contains("\"passed\"") && text.contains("\"reason\""));
        }
        let expected_url = match provider {
            Provider::Anthropic => "http://localhost:9/v1/messages",
            Provider::Jev => "http://localhost:9/v1/systemone",
            Provider::JevCloudflare => &format!("http://localhost:9/accounts/{ACCOUNT}/ai/run"),
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
        model: "",
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
            reason: "The cart badge shows 2 items.".into(),
            usage: Usage {
                input_tokens: 1543,
                output_tokens: 61
            }
        }
    );
    assert_eq!(
        read(Provider::OpenAi, "openai.json"),
        Judgement {
            passed: false,
            reason: "The cart is empty.".into(),
            usage: Usage {
                input_tokens: 1117,
                output_tokens: 46
            }
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
            reason: "The Save button is visible.".into(),
            usage: Usage {
                input_tokens: 790,
                output_tokens: 31
            }
        }
    );
    assert_eq!(
        read(Provider::Gemini, "gemini.json").usage,
        Usage {
            input_tokens: 1290,
            output_tokens: 24
        }
    );
    assert_eq!(
        read(Provider::Ollama, "ollama.json").usage,
        Usage {
            input_tokens: 812,
            output_tokens: 19
        }
    );
    assert_eq!(
        read(Provider::Jev, "jev.json"),
        Judgement {
            passed: false,
            reason: "JEV is 96% sure the screenshot does not meet the expectation.".into(),
            usage: Usage {
                input_tokens: 1862,
                output_tokens: 1
            }
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
        Provider::JevCloudflare,
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

/// The head and body of the request the reader gets, as one text.
fn read_request(reader: &mut BufReader<TcpStream>) -> String {
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
    head
}

/// A server on this computer that promises all of `body` and drops the connection after the
/// first `sent` bytes of it; its address.
fn serve_cut(body: &str, sent: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let body = body.to_owned();
    thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        read_request(&mut reader);
        let mut stream = reader.into_inner();
        let _ = write!(
            stream,
            "HTTP/1.1 200 X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            &body[..sent]
        );
    });
    address
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
            seen.lock().unwrap().push(read_request(&mut reader));
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
fn a_question_can_ask_another_model_of_the_same_provider() {
    let server = serve(vec![(200, fixture("openai.json")), (404, "{}".into())]);
    let c = client(Provider::OpenAi, &server.address);
    let judgement = c
        .judge(&Question {
            model: " gpt-5 ",
            ..question()
        })
        .unwrap();
    assert_eq!(judgement.usage.output_tokens, 46);
    let sent = server.requests.lock().unwrap()[0].clone();
    assert!(sent.contains(r#""model":"gpt-5""#));
    assert_eq!(
        c.judge(&Question {
            model: "gpt-0",
            ..question()
        }),
        Err(VisionError::UnknownModel {
            model: "gpt-0".into()
        })
    );
    assert_eq!(c.model(), "vision-model");
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

#[test]
fn tests_can_name_every_provider() {
    let ids: Vec<&str> = Provider::ALL.iter().map(|p| p.id()).collect();
    assert_eq!(ids, jevision_core::AI_PROVIDERS);
}

#[test]
fn jev_finds_an_element_by_description() {
    let found = fixture("jev-box.json");
    let absent = found.replace("0.97", "0.2");
    let server = serve(vec![(200, found), (200, absent)]);
    let c = client(Provider::Jev, &server.address);
    let search = c.locate(&SHOT_PICTURE, " the blue Save button ").unwrap();
    // The tokens count too, whether or not the element is there.
    assert_eq!(
        search.usage,
        Usage {
            input_tokens: 1874,
            output_tokens: 6
        }
    );
    let located = search.found.unwrap();
    assert_eq!(
        located.area,
        Rect {
            x: 0.5,
            y: 0.25,
            width: 0.7 - 0.5,
            height: 0.5,
        }
    );
    assert_eq!(located.confidence, 0.92);
    // A box always comes back; whether the element is there is a question of its own.
    let absent = c.locate(&SHOT_PICTURE, "the blue Save button").unwrap();
    assert_eq!(absent.found, None);
    assert_eq!(absent.usage.input_tokens, 1874);

    let sent = server.requests.lock().unwrap()[0].clone();
    assert!(sent.starts_with("POST /v1/systemone "));
    let lower = sent.to_ascii_lowercase();
    assert!(lower.contains(&format!(
        "authorization: bearer {}",
        KEY.to_ascii_lowercase()
    )));
    assert!(lower.contains("typesafe-danger-zone-images: true"));
    let body: Value = serde_json::from_str(&sent[sent.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(images(&body), [SHOT]);
    assert_eq!(body["questions"]["visible"]["type"], "noul");
    assert_eq!(body["questions"]["where"]["type"], "box");
    for question in ["visible", "where"] {
        let instructions = body["questions"][question]["instructions"]
            .as_str()
            .unwrap();
        assert!(instructions.contains(JEV_SCREENSHOT), "{instructions}");
        assert!(
            instructions.contains("in the screenshot jevision-screenshot: the blue Save button")
        );
    }
    assert!(body["questions"]["where"].get("criteria").is_none());
}

#[test]
fn a_box_on_the_padding_of_a_long_screenshot_is_off_the_screen() {
    let fill = (0.5, 1.0);
    let answer = |left: f64, right: f64| {
        format!(
            r#"{{"answers": {{"visible": {{"noul": 0.9}}, "where": {{"box": {{"top": 0.1, "bottom": 0.3, "left": {left}, "right": {right}}}}}}}}}"#
        )
    };
    let inside = located(Provider::Jev, &answer(0.1, 0.4), fill)
        .unwrap()
        .found
        .unwrap();
    assert_eq!(inside.area.x, 0.2);
    assert!((inside.area.width - 0.6).abs() < 1e-9);
    assert_eq!(inside.confidence, 0.9);
    assert_eq!(
        located(Provider::Jev, &answer(0.6, 0.9), fill)
            .unwrap()
            .found,
        None
    );
    assert!(matches!(
        located(Provider::Jev, &answer(0.4, 0.1), fill),
        Err(VisionError::BadAnswer { .. })
    ));
}

#[test]
fn only_jev_finds_elements_and_only_from_version_1_14() {
    for provider in Provider::ALL.into_iter().filter(|p| !p.is_jev()) {
        let c = Client::new(
            config(provider, "http://127.0.0.1:9"),
            Duration::from_secs(1),
        );
        assert_eq!(
            c.locate(&SHOT_PICTURE, "the Save button"),
            Err(VisionError::CannotLocate)
        );
    }
    let locate = |c: Config| {
        Client::new(c, Duration::from_secs(1))
            .locate(&SHOT_PICTURE, "the Save button")
            .unwrap_err()
    };
    let jev = |model: &str| Config {
        model: model.into(),
        ..config(Provider::Jev, "http://127.0.0.1:9")
    };
    assert_eq!(locate(jev("jev-1.13.0")), VisionError::ModelCannotLocate);
    for model in [
        "jev-1.14.0",
        "jev-2.0.1",
        "jev-latest",
        "jev-preview",
        "typesafe/jev-1.14.0",
    ] {
        assert!(jev_locates(model), "{model}");
    }
    assert!(!jev_locates("typesafe/jev-1.13.0"));
    let on_cloudflare = |model: &str| Config {
        model: model.into(),
        ..config(Provider::JevCloudflare, "http://127.0.0.1:9")
    };
    assert_eq!(
        locate(on_cloudflare("typesafe/jev-1.13.0")),
        VisionError::ModelCannotLocate
    );
    assert_eq!(
        locate(Config {
            pictures: false,
            ..jev("jev-latest")
        }),
        VisionError::NoPictures
    );
    assert_eq!(
        locate(Config {
            key: None,
            ..jev("jev-latest")
        }),
        VisionError::NoKey
    );
}

#[test]
fn jev_says_how_sure_it_is_in_each_language() {
    use jevision_l10n::tr_in;
    assert_eq!(
        tr_in!("en", "vision.jev.failed", 96),
        "JEV is 96% sure the screenshot does not meet the expectation."
    );
    assert_eq!(
        tr_in!("ru", "vision.jev.failed", 96),
        "JEV на 96\u{a0}% уверен, что снимок экрана не соответствует ожиданию."
    );
    assert_eq!(
        tr_in!(
            "ru",
            "vision.error.onlyChecksScreenshots",
            Provider::Jev.name()
        ),
        "JEV (TypeSafe AI) умеет только проверять снимки экрана"
    );
}

#[test]
fn jev_through_cloudflare_sends_what_typesafe_gets_wrapped_with_the_model_beside_it() {
    let direct = request(&config(Provider::Jev, ""), &question()).1;
    let (url, body) = request(&config(Provider::JevCloudflare, ""), &question());
    assert_eq!(
        url,
        format!("https://api.cloudflare.com/client/v4/accounts/{ACCOUNT}/ai/run")
    );
    let mut input = direct.clone();
    input.as_object_mut().unwrap().remove("model");
    assert_eq!(body, json!({ "model": "vision-model", "input": input }));

    let (direct_url, direct, direct_fill) =
        locate_request(&config(Provider::Jev, ""), &SHOT_PICTURE, "the Save button");
    let (url, body, fill) = locate_request(
        &config(Provider::JevCloudflare, ""),
        &SHOT_PICTURE,
        "the Save button",
    );
    assert_eq!(direct_url, "https://api.typesafe.ai/v1/systemone");
    assert!(url.ends_with(&format!("/accounts/{ACCOUNT}/ai/run")));
    assert_eq!(fill, direct_fill);
    let mut input = direct.clone();
    input.as_object_mut().unwrap().remove("model");
    assert_eq!(body, json!({ "model": "vision-model", "input": input }));

    // The account goes into the address as the dashboard shows it, whatever the case typed.
    let shouted = Config {
        account_id: format!(" {} ", ACCOUNT.to_ascii_uppercase()),
        ..config(Provider::JevCloudflare, "")
    };
    assert!(request(&shouted, &question()).0.contains(ACCOUNT));

    // The token, and nothing TypeSafe AI asks for.
    assert_eq!(
        headers(&config(Provider::JevCloudflare, "")),
        [("authorization", format!("Bearer {KEY}"))]
    );
    let p = Provider::JevCloudflare;
    assert_eq!(
        (p.id(), p.name(), p.key_name(), p.account_name()),
        (
            "jev-cloudflare",
            "JEV via Cloudflare",
            Some("CLOUDFLARE_API_TOKEN"),
            Some("CLOUDFLARE_ACCOUNT_ID")
        )
    );
    assert!(p.needs_key() && !p.address_editable() && p.may_lack_pictures() && !p.converses());
    assert_eq!(p.default_model(), "typesafe/jev-1.14.0");
    assert!(
        Provider::ALL
            .into_iter()
            .filter(|p| p.account_name().is_some())
            .eq([Provider::JevCloudflare])
    );
}

/// The request body of a request the server saw, parsed.
fn sent_body(request: &str) -> Value {
    serde_json::from_str(&request[request.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

#[test]
fn recorded_answers_of_cloudflare_are_read_from_their_wrapping() {
    let judged = answer(Provider::JevCloudflare, &fixture("cloudflare-jev.json")).unwrap();
    assert!(judged.passed && judged.reason.contains("72%"), "{judged:?}");
    assert_eq!(
        judged.usage,
        Usage {
            input_tokens: 7761,
            output_tokens: 20
        }
    );

    let search = located(
        Provider::JevCloudflare,
        &fixture("cloudflare-jev-box.json"),
        (1.0, 1.0),
    )
    .unwrap();
    assert_eq!(
        search.usage,
        Usage {
            input_tokens: 11102,
            output_tokens: 36
        }
    );
    let found = search.found.unwrap();
    assert_eq!((found.area.x, found.area.y), (0.6, 0.75));
    assert!((found.area.width - 0.27).abs() < 1e-9 && (found.area.height - 0.11).abs() < 1e-9);
    assert_eq!(found.confidence, 0.25);

    // Each route reads only its own shape: TypeSafe AI's answer is not wrapped, Cloudflare's is.
    let bad = |e| matches!(e, Err(VisionError::BadAnswer { .. }));
    for unwrapped in [fixture("jev.json"), fixture("jev-box.json")] {
        assert!(bad(answer(Provider::JevCloudflare, &unwrapped).map(|_| ())));
        assert!(bad(located(
            Provider::JevCloudflare,
            &unwrapped,
            (1.0, 1.0)
        )
        .map(|_| ())));
    }
    for wrapped in [
        fixture("cloudflare-jev.json"),
        fixture("cloudflare-jev-box.json"),
    ] {
        assert!(bad(answer(Provider::Jev, &wrapped).map(|_| ())));
        assert!(bad(located(Provider::Jev, &wrapped, (1.0, 1.0)).map(|_| ())));
    }
    assert!(!answer(Provider::Jev, &fixture("jev.json")).unwrap().passed);
    assert!(
        located(Provider::Jev, &fixture("jev-box.json"), (1.0, 1.0))
            .unwrap()
            .found
            .is_some()
    );
}

#[test]
fn jev_through_cloudflare_over_http() {
    let server = serve(vec![
        (200, fixture("cloudflare-jev.json")),
        (200, fixture("cloudflare-jev-box.json")),
    ]);
    let c = client(Provider::JevCloudflare, &server.address);
    assert!(c.judge(&question()).unwrap().passed);
    assert!(
        c.locate(&SHOT_PICTURE, "the blue Save button")
            .unwrap()
            .found
            .is_some()
    );

    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    for sent in &requests {
        assert!(
            sent.starts_with(&format!("POST /accounts/{ACCOUNT}/ai/run ")),
            "{sent}"
        );
        let lower = sent.to_ascii_lowercase();
        assert!(lower.contains(&format!(
            "authorization: bearer {}",
            KEY.to_ascii_lowercase()
        )));
        assert!(!lower.contains("typesafe-danger-zone-images"), "{sent}");
        let text = &sent[sent.find("\r\n\r\n").unwrap() + 4..];
        assert!(!text.contains(KEY), "the token goes in a header only");
        let body = sent_body(sent);
        assert_eq!(body["model"], "vision-model");
        assert!(body.get("questions").is_none() && body["input"]["questions"].is_object());
        assert!(images(&body).contains(&SHOT.to_vec()));
    }
}

#[test]
fn cloudflare_failures_say_what_to_fix() {
    let no_balance = fixture("cloudflare-no-balance.json");
    let denied = fixture("cloudflare-denied.json");
    let model = VisionError::UnknownModel {
        model: "vision-model".into(),
    };
    let failed = |status, message: &str| VisionError::Service {
        status,
        message: message.into(),
    };
    let cases = [
        (402, no_balance, VisionError::CloudflareNoBalance, true),
        (401, denied.clone(), VisionError::CloudflareAccess, false),
        (403, denied, VisionError::CloudflareAccess, false),
        (
            400,
            fixture("cloudflare-image-size.json"),
            VisionError::PictureSize,
            false,
        ),
        (404, fixture("cloudflare-unknown-model.json"), model, false),
        (
            400,
            r#"{"result":null,"success":false,"errors":[{"code":7000,"message":"Bad input"}]}"#
                .into(),
            failed(400, "Bad input"),
            false,
        ),
        (
            500,
            fixture("cloudflare-failed.json"),
            failed(500, "Model execution failed (Internal Error)"),
            true,
        ),
        // Said with a status of 200: the service failing all the same.
        (
            200,
            fixture("cloudflare-failed.json"),
            failed(502, "Model execution failed (Internal Error)"),
            true,
        ),
        (
            200,
            fixture("cloudflare-no-balance.json"),
            failed(
                502,
                "Insufficient balance; add money to your gateway or use BYOK",
            ),
            true,
        ),
    ];
    for (status, body, expected, unavailable) in cases {
        let server = serve(vec![(status, body.clone()), (status, body)]);
        let c = client(Provider::JevCloudflare, &server.address);
        for error in [
            c.judge(&question()).unwrap_err(),
            c.locate(&SHOT_PICTURE, "the Save button").unwrap_err(),
        ] {
            assert_eq!(error, expected, "status {status}");
            assert_eq!(error.unavailable(), unavailable, "status {status}");
        }
    }

    // Only Cloudflare's own failures are read this way: TypeSafe AI's 402 stays an error of its
    // service, and a 401 there is still the key.
    let direct = serve(vec![(402, "{}".into()), (401, "{}".into())]);
    let c = client(Provider::Jev, &direct.address);
    assert_eq!(c.judge(&question()).unwrap_err(), failed(402, "{}"));
    assert_eq!(c.judge(&question()).unwrap_err(), VisionError::BadKey);
}

#[test]
fn a_cut_answer_of_cloudflare_is_an_error_not_a_verdict() {
    let body = fixture("cloudflare-jev.json").trim_end().to_owned();
    // The connection drops with part of the answer on the wire.
    for sent in [0, body.len() / 2, body.len() - 1] {
        let address = serve_cut(&body, sent);
        let error = client(Provider::JevCloudflare, &address)
            .judge(&question())
            .unwrap_err();
        assert_eq!(error, VisionError::Unreachable { address }, "{sent} bytes");
        assert!(error.unavailable());
    }
    // The answer is whole on the wire and cut in itself, wherever.
    for cut in 0..body.len() {
        assert!(
            matches!(
                answer(Provider::JevCloudflare, &body[..cut]),
                Err(VisionError::BadAnswer { .. })
            ),
            "cut at {cut}"
        );
    }
    let box_body = fixture("cloudflare-jev-box.json").trim_end().to_owned();
    for cut in 0..box_body.len() {
        assert!(
            matches!(
                located(Provider::JevCloudflare, &box_body[..cut], (1.0, 1.0)),
                Err(VisionError::BadAnswer { .. })
            ),
            "cut at {cut}"
        );
    }
    let server = serve(vec![(200, body[..body.len() / 2].to_owned())]);
    assert!(matches!(
        client(Provider::JevCloudflare, &server.address).judge(&question()),
        Err(VisionError::BadAnswer { .. })
    ));
}

#[test]
fn jev_through_cloudflare_needs_an_account_id_before_anything_is_sent() {
    let server = serve(vec![
        (200, fixture("cloudflare-jev.json")),
        (200, fixture("cloudflare-jev.json")),
    ]);
    let with = |account: &str| {
        Client::new(
            Config {
                account_id: account.into(),
                ..config(Provider::JevCloudflare, &server.address)
            },
            Duration::from_secs(5),
        )
    };
    assert_eq!(with("  ").judge(&question()), Err(VisionError::NoAccount));
    assert_eq!(
        with("").locate(&SHOT_PICTURE, "the Save button"),
        Err(VisionError::NoAccount)
    );
    assert_eq!(with("").test(), Err(VisionError::NoAccount));
    let wrong = [
        "abc".to_owned(),
        "g".repeat(32),
        format!("{ACCOUNT}0"),
        format!("dash.cloudflare.com/{ACCOUNT}"),
        KEY.to_owned(),
    ];
    for account in &wrong {
        let error = with(account).judge(&question()).unwrap_err();
        assert_eq!(error, VisionError::BadAccount, "{account}");
        // What was typed may be a secret that landed in the wrong field.
        assert!(!format!("{error} {error:?}").contains(account));
    }
    assert!(server.requests.lock().unwrap().is_empty());

    // As the dashboard shows it, in any case and with spaces around, it is sent.
    assert!(
        with(&format!(" {} ", ACCOUNT.to_ascii_uppercase()))
            .judge(&question())
            .is_ok()
    );
    assert!(with(ACCOUNT).judge(&question()).is_ok());
    assert_eq!(server.requests.lock().unwrap().len(), 2);

    // The others have no account to name.
    for provider in Provider::ALL {
        let c = Client::new(
            Config {
                account_id: String::new(),
                ..config(provider, "")
            },
            Duration::from_secs(1),
        );
        assert_eq!(
            c.ready().is_err(),
            provider == Provider::JevCloudflare,
            "{provider:?}"
        );
    }
}

#[test]
fn the_token_never_shows_in_what_cloudflare_says() {
    let said = |text: &str| {
        format!(
            r#"{{"result":null,"success":false,"errors":[{{"code":7000,"message":"{text}"}}],"messages":[]}}"#
        )
    };
    let echo = format!("Bad input from {KEY}.");
    let server = serve(vec![
        (400, said(&echo)),
        (429, said(&echo)),
        (500, said(&echo)),
        (200, said(&echo)),
        (
            200,
            format!(r#"{{"result":{{"result":{{"answers":"{KEY}"}}}},"success":true}}"#),
        ),
    ]);
    let c = client(Provider::JevCloudflare, &server.address);
    let errors: Vec<VisionError> = (0..5).map(|_| c.judge(&question()).unwrap_err()).collect();
    assert_eq!(
        errors[0],
        VisionError::Service {
            status: 400,
            message: format!("Bad input from {MASK}.")
        }
    );
    assert!(matches!(&errors[1], VisionError::RateLimited { message } if message.contains(MASK)));
    assert!(matches!(&errors[4], VisionError::BadAnswer { answer } if answer.contains(MASK)));
    for e in &errors {
        assert!(!format!("{e} {e:?}").contains(KEY), "{e:?}");
    }
    assert!(!format!("{:?}", c.config()).contains(KEY));
}

#[test]
fn small_pictures_are_enlarged_for_cloudflare_before_they_are_sent() {
    let png = image::Image {
        width: 200,
        height: 120,
        rgba: [200, 30, 30, 255].repeat(200 * 120),
    }
    .encode()
    .unwrap();
    let small = MaskedImage::from_png(&png, &[]).unwrap();
    let server = serve(vec![
        (200, fixture("cloudflare-jev.json")),
        (200, fixture("cloudflare-jev-box.json")),
    ]);
    let c = client(Provider::JevCloudflare, &server.address);
    c.judge(&Question {
        screenshot: &small,
        reference: None,
        ..question()
    })
    .unwrap();
    c.locate(&small, "the red area").unwrap();
    for sent in server.requests.lock().unwrap().iter() {
        let pictures = images(&sent_body(sent));
        assert_eq!(pictures.len(), 1);
        let picture = image::Image::decode(&pictures[0]).unwrap();
        // Cloudflare answers "Invalid image dimensions" for a side under 280; this is the shape
        // of the small one, grown as a whole.
        assert_eq!((picture.width, picture.height), (467, 280));
    }
}

/// Asks the real JEV 1.14 through Cloudflare Workers AI about a drawn window: a yes, a no, a
/// search and the settings check, four paid requests, and the two failures a wrong token and a
/// wrong account give, which cost nothing. Run by hand, the secrets in the environment:
/// `cargo test -p jevision-vision -- --ignored --nocapture real_jev`.
#[test]
#[ignore = "asks the real JEV through Cloudflare, which costs money and needs CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID"]
fn real_jev_through_cloudflare() {
    let (Ok(key), Ok(account)) = (
        std::env::var("CLOUDFLARE_API_TOKEN"),
        std::env::var("CLOUDFLARE_ACCOUNT_ID"),
    ) else {
        eprintln!(
            "skipped: set CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID to ask the real JEV through Cloudflare"
        );
        return;
    };
    // A grey window with a title bar and a blue button with a white label at the bottom right.
    let (width, height) = (640usize, 480usize);
    let mut rgba = [235, 235, 235, 255].repeat(width * height);
    let mut fill = |x: std::ops::Range<usize>, y: std::ops::Range<usize>, rgb: [u8; 3]| {
        for y in y {
            for x in x.clone() {
                rgba[(y * width + x) * 4..][..3].copy_from_slice(&rgb);
            }
        }
    };
    fill(60..300, 80..120, [60, 60, 60]);
    fill(400..560, 360..420, [10, 90, 240]);
    fill(440..520, 385..395, [255, 255, 255]);
    let png = image::Image {
        width: width as u32,
        height: height as u32,
        rgba,
    }
    .encode()
    .unwrap();
    let picture = MaskedImage::from_png(&png, &[]).unwrap();
    let config = Config {
        provider: Provider::JevCloudflare,
        model: Provider::JevCloudflare.default_model().into(),
        address: String::new(),
        key: Some(key),
        pictures: true,
        account_id: account,
    };
    let c = Client::new(config.clone(), TIMEOUT);
    let started = std::time::Instant::now();
    let ask = |expect| {
        let began = std::time::Instant::now();
        let judgement = c.judge(&Question {
            expect,
            screenshot: &picture,
            reference: None,
            model: "",
        });
        eprintln!("{:.1}s: {judgement:?}", began.elapsed().as_secs_f32());
        judgement
    };
    let yes = ask("A blue button at the bottom right of a grey window.");
    let no = ask("A red circle on a black background.");
    let began = std::time::Instant::now();
    let search = c.locate(&picture, "the blue button");
    eprintln!("{:.1}s: {search:?}", began.elapsed().as_secs_f32());
    let check = c.test();
    eprintln!("settings check: {check:?}");

    let yes = yes.unwrap();
    assert!(yes.passed && yes.usage.input_tokens > 0, "{yes:?}");
    assert!(!no.unwrap().passed);
    let search = search.unwrap();
    assert!(search.usage.input_tokens > 0, "{search:?}");
    let found = search.found.expect("JEV sees the button");
    let (x, y) = (
        found.area.x + found.area.width / 2.0,
        found.area.y + found.area.height / 2.0,
    );
    // The button: 400..560 of 640 across, 360..420 of 480 down.
    assert!(
        (0.625..0.875).contains(&x) && (0.75..0.875).contains(&y),
        "{found:?}"
    );
    check.unwrap();
    eprintln!("all in {:.1}s", started.elapsed().as_secs_f32());

    // What a person sees for a wrong token and for another account's number.
    let wrong = |config: Config| {
        Client::new(config, TIMEOUT)
            .judge(&Question {
                expect: "A blue button.",
                screenshot: &picture,
                reference: None,
                model: "",
            })
            .unwrap_err()
    };
    let token = wrong(Config {
        key: Some("x".repeat(40)),
        ..config.clone()
    });
    assert_eq!(token, VisionError::CloudflareAccess);
    let other = wrong(Config {
        account_id: "0".repeat(32),
        ..config
    });
    assert_eq!(other, VisionError::CloudflareAccess);
}
