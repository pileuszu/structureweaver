//! Exercise the real HTTP client against bounded local HTTP mocks, never a model.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use structureweaver::llama::{LlamaClient, LlamaConfig};

#[derive(Debug)]
struct Request {
    head: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Request {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut data = Vec::new();
    let header_end = loop {
        if let Some(index) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break index + 4;
        }
        assert!(data.len() < 64 * 1024, "unexpected oversized HTTP headers");
        let mut chunk = [0; 2048];
        let count = stream.read(&mut chunk).expect("request headers timed out");
        assert_ne!(count, 0, "connection closed before HTTP headers");
        data.extend_from_slice(&chunk[..count]);
    };
    let head = String::from_utf8(data[..header_end].to_vec()).unwrap();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    assert!(length < 1024 * 1024, "unexpected oversized request body");
    while data.len() < header_end + length {
        let mut chunk = [0; 4096];
        let count = stream.read(&mut chunk).expect("request body timed out");
        assert_ne!(count, 0, "connection closed before request body");
        data.extend_from_slice(&chunk[..count]);
    }
    Request {
        head,
        body: data[header_end..header_end + length].to_vec(),
    }
}

fn server(responses: Vec<(&'static str, String)>) -> (LlamaClient, JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("local HTTP mock must bind");
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "no client request within 5 seconds"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("mock accept failed: {error}"),
                }
            };
            requests.push(read_request(&mut stream));
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            stream.flush().unwrap();
        }
        requests
    });
    let client = LlamaClient::new(LlamaConfig {
        base_url: format!("http://{address}/v1/"),
        model: "local-vision-test".into(),
        timeout_secs: 3,
    })
    .unwrap();
    (client, handle)
}

fn envelope(content: &str, finish_reason: &str) -> String {
    json!({"choices":[{"finish_reason":finish_reason,"message":{"content":content}}]}).to_string()
}

#[test]
fn models_prioritizes_nested_loaded_status_and_preserves_other_models() {
    let body = json!({"data":[
        {"id":"unloaded-a","status":{"value":"unloaded"}},
        {"id":"nested-loaded","status":{"value":"loaded"}},
        {"id":"string-loaded","status":"loaded"},
        {"id":"boolean-loaded","loaded":true},
        {"id":"unloaded-b"},
        {"status":"loaded"}
    ]})
    .to_string();
    let (client, handle) = server(vec![("200 OK", body)]);
    let models = client.models().unwrap();
    let requests = handle.join().unwrap();
    assert_eq!(
        models,
        [
            "nested-loaded",
            "string-loaded",
            "boolean-loaded",
            "unloaded-a",
            "unloaded-b"
        ]
    );
    assert!(requests[0].head.starts_with("GET /v1/models HTTP/1.1\r\n"));
    assert!(requests[0].body.is_empty());
}

#[test]
fn structure_posts_image_schema_and_model_and_parses_structured_content() {
    let schema =
        json!({"type":"object","properties":{"tables":{"type":"array"}},"required":["tables"]});
    let layout =
        json!({"tables":[{"name":"Customers","bounds":{"top":1,"left":1,"bottom":3,"right":2}}]});
    let body = envelope(&format!("```json\n{layout}\n```"), "stop");
    let (client, handle) = server(vec![("200 OK", body)]);
    let result = client
        .structure(
            "Use absolute cell coordinates.",
            b"\x89PNG\r\n",
            schema.clone(),
        )
        .unwrap();
    let requests = handle.join().unwrap();
    assert_eq!(result, layout);
    assert!(
        requests[0]
            .head
            .starts_with("POST /v1/chat/completions HTTP/1.1\r\n")
    );
    let request: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(request["model"], "local-vision-test");
    assert_eq!(
        request["messages"][1]["content"][0]["text"],
        "Use absolute cell coordinates."
    );
    assert_eq!(request["messages"][1]["content"][1]["type"], "image_url");
    assert_eq!(
        request["messages"][1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,iVBORw0K"
    );
    assert_eq!(request["response_format"]["type"], "json_schema");
    assert_eq!(request["response_format"]["json_schema"]["strict"], true);
    assert_eq!(request["response_format"]["json_schema"]["schema"], schema);
    assert_eq!(request["stream"], false);
}

#[test]
fn invalid_json_truncated_output_and_http_errors_are_explicit_failures() {
    let (client, handle) = server(vec![
        ("200 OK", "this is not HTTP JSON".into()),
        ("200 OK", envelope("{bad output", "stop")),
        // Even syntactically valid partial output must fail when the budget was exhausted.
        ("200 OK", envelope("{\"tables\":[]}", "length")),
        ("503 Service Unavailable", "model unavailable".into()),
    ]);
    let errors: Vec<_> = (0..4)
        .map(|_| {
            client
                .structure("test", b"png", json!({"type":"object"}))
                .unwrap_err()
                .to_string()
        })
        .collect();
    let requests = handle.join().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(errors[0].contains("invalid HTTP JSON"), "{}", errors[0]);
    assert!(errors[1].contains("JSON format"), "{}", errors[1]);
    assert!(errors[2].contains("exceeded token budget"), "{}", errors[2]);
    assert!(
        errors[3].contains("503") && errors[3].contains("model unavailable"),
        "{}",
        errors[3]
    );
}

#[test]
fn remote_endpoints_are_rejected_before_any_request() {
    for base_url in [
        "https://example.com/v1",
        "http://192.168.1.2:9931/v1",
        "http://127.0.0.1.example.com/v1",
    ] {
        let result = LlamaClient::new(LlamaConfig {
            base_url: base_url.into(),
            ..LlamaConfig::default()
        });
        let error = match result {
            Ok(_) => panic!("remote endpoint accepted: {base_url}"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("loopback"));
    }
}
