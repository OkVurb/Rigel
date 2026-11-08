//! Roadmap 26 (Per-project OpenRouter preferences) and 28 (custom OpenAI-compatible door).

use rigel_core::agent::custom::{self, CustomDoor, HeaderPair};
use rigel_core::agent::openrouter::RoutingPolicy;
use rigel_core::credentials::MemoryCredentialStore;

#[test]
fn a_project_policy_is_read_and_described_before_the_turn() {
    let root = std::env::temp_dir().join(format!("rigel-26-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".rigel")).unwrap();
    std::fs::write(
        root.join(".rigel/routing.json"),
        r#"{"order":["anthropic","openai"],"allow_fallbacks":false,"quantizations":["fp16"]}"#,
    )
    .unwrap();

    let policy = RoutingPolicy::load(&root);
    assert_eq!(policy.order, vec!["anthropic", "openai"]);
    assert!(!policy.allow_fallbacks);
    assert_eq!(policy.quantizations, vec!["fp16"]);
    assert_eq!(
        policy.describe(),
        "order anthropic -> openai · fallbacks off · quantizations fp16"
    );

    let missing = RoutingPolicy::load(&root.join("no-such-project"));
    assert!(missing.is_default());
    assert_eq!(missing.describe(), "default (no order, fallbacks on)");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_custom_door_is_validated_and_only_its_header_names_come_back() {
    let mut door = CustomDoor {
        name: "gateway".to_string(),
        base_url: "https://gateway.example/v1".to_string(),
        headers: vec![HeaderPair {
            name: "x-gateway-token".to_string(),
            value: "secret".to_string(),
        }],
        models: vec!["model-a".to_string()],
    };
    assert!(door.validate().is_ok());

    let masked = door.masked();
    let text = serde_json::to_string(&masked).unwrap();
    assert!(!text.contains("secret"), "a header value left the keychain");
    assert_eq!(masked.header_names, vec!["x-gateway-token"]);

    door.base_url = "http://gateway.example/v1".to_string();
    assert!(
        door.validate().is_err(),
        "remote plain http must be refused"
    );
    door.base_url = "http://127.0.0.1:8081/v1".to_string();
    assert!(door.validate().is_ok(), "loopback http is the local case");
}

#[test]
fn the_saved_door_is_read_from_the_keychain_record_without_a_second_store() {
    let door = CustomDoor {
        name: "gateway".to_string(),
        base_url: "https://gateway.example/v1".to_string(),
        headers: vec![HeaderPair {
            name: "x-gateway-token".to_string(),
            value: "secret".to_string(),
        }],
        models: vec!["model-a".to_string(), "model-b".to_string()],
    };
    let encoded = door.encode().unwrap();
    let store = MemoryCredentialStore::with(custom::ID, &encoded);
    let loaded = custom::load_from(&store).unwrap().expect("the door loads");
    assert_eq!(loaded, door);

    let empty = MemoryCredentialStore::new();
    assert!(custom::load_from(&empty).unwrap().is_none());
}

#[tokio::test]
async fn the_probe_lists_the_door_models_before_anything_is_saved() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        let _ = socket.read(&mut buffer).await;
        let body = r#"{"data":[{"id":"alpha"},{"id":"beta"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
    });

    let base = format!("http://{address}/v1");
    let headers = vec![HeaderPair {
        name: "authorization".to_string(),
        value: "Bearer test".to_string(),
    }];
    let models = custom::probe(&base, &headers).await.unwrap();
    assert_eq!(models, vec!["alpha", "beta"]);
}
