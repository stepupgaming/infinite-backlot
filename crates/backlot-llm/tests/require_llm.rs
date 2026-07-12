//! REQUIRE-LLM mode must fail clearly and never silently fall back to the
//! deterministic director. This is the load-bearing guarantee from the PRD:
//! when `require_llm = true` and the configured endpoint is unreachable or
//! returns an unusable response, production stops with an error instead of
//! quietly shipping a fallback-authored episode.

use backlot_core::config::{DirectorConfig, LlmConfig};
use backlot_core::director::DirectorContext;
use backlot_core::world::build_default_world;
use backlot_core::author::EpisodeAuthor;
use backlot_llm::LlmAuthor;

#[test]
fn require_llm_fails_clearly_without_silent_fallback() {
    // Point at a port that refuses connections so the request fails fast.
    let llm = LlmConfig {
        base_url: "http://127.0.0.1:1/v1".into(),
        model: "nope".into(),
        ..Default::default()
    };
    let mut dir = DirectorConfig::default();
    dir.require_llm = true;

    let author = LlmAuthor::new(llm, dir).expect("client construction must not fail");
    let ctx = DirectorContext {
        world: build_default_world(),
        episode_number: 1,
        seed: 1,
        target_duration: 55.0,
        recent_summaries: vec![],
        tone: vec![],
    };

    let res = author.author(&ctx);
    assert!(
        res.is_err(),
        "require_llm must NOT silently fall back; got Ok plan"
    );
}
