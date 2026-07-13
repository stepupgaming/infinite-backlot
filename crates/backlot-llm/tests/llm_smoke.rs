//! Isolated smoke test for the OpenAI-compatible client (no Bevy, no render).
//! Verifies a real structured call against the local server returns quickly.

use backlot_core::config::Config;
use backlot_llm::client::LlmClient;

#[tokio::test]
async fn smoke_llm_structured() {
    let config = Config::load("../../data/config.toml").expect("load config");
    let client = LlmClient::new(config.llm.clone()).expect("client");

    let schema = r#"{"type":"object","properties":{"ok":{"type":"boolean"},"note":{"type":"string"}},"required":["ok","note"]}"#;
    let start = std::time::Instant::now();
    let res = client
        .chat_structured(
            "You are a JSON emitter. Respond ONLY with the requested JSON object.",
            "Return ok=true and a short note about the hallway.",
            "Smoke",
            schema,
            1,
        )
        .await;
    let elapsed = start.elapsed();
    match &res {
        Ok(s) => println!("OK in {elapsed:?}: {}", &s[..s.len().min(400)]),
        Err(e) => println!("ERR in {elapsed:?}: {e}"),
    }
    res.expect("llm structured call must succeed");
}
