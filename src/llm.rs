use bevy::{asset::Asset, log::info, reflect::TypePath};
use kalosm::language::*;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

static LLM: OnceCell<Llama> = OnceCell::const_new();
async fn get_llm() -> &'static Llama {
    LLM.get_or_init(|| async {
        let llm = Llama::builder()
            .with_source(
                LlamaSource::new(FileSource::local(
                    "assets/llm/llama/Llama-3.2-3B-Instruct-Q4_K_M.gguf".into(),
                ))
                .with_tokenizer(FileSource::Local("assets/llm/llama/tokenizer.json".into()))
                .with_config(FileSource::local("assets/llm/llama/config.json".into()))
                .with_group_query_attention(1),
            )
            .build()
            .await
            .unwrap();
        info!("LLM Loaded");
        llm
    }).await
}

pub async fn settlement_names(ctx: SettlementNameCtx, temp: f32) -> anyhow::Result<Vec<String>> {
    let params = GenerationParameters::default()
        .with_max_length(48) // ~enough for 3 short lines
        .with_temperature(temp)
        .with_top_p(0.9)
        .with_repetition_penalty(1.12);
    let llm  = get_llm().await;
    let prompt = r#"You output ONLY a single JSON object that conforms EXACTLY to the provided JSON Schema.
Absolutely no extra text, no explanations, no examples, no code fences.

Content rules for each name:
- 1-3 words.
- In theme with the Civilisation Name
- Only these characters: letters, numbers, spaces, . ! ? and (optionally) '.
- Do not echo template tokens or placeholders (e.g., @handle, #Tag, %TOKEN%).
- Must be UTF-8 compliant.

If any name would violate the rules, replace it with a different name that complies.
"#;
    let task = llm.task(prompt).typed::<SettlementNamePack>();
    let stream = task
        .run(serde_json::ser::to_string(&ctx)?)
        .with_sampler(params.clone());
    let names = stream.await.unwrap();
    info!("Names Generated: {:?}",names.settlement_names);
    Ok(names.settlement_names.iter().map(|n| n.text.clone()).collect())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettlementNameCtx {
    pub civilisation_name: String,
}
#[derive(Parse, Schema, Clone, Debug, Serialize, Deserialize)]
struct SettlementNamePack {
    settlement_names: [SettlementName; 10],
}
#[derive(Parse, Schema, Clone, Debug, Serialize, Deserialize)]
struct SettlementName {
    // You can tighten this regex to your exact rules
    #[parse(pattern = r"[A-Za-z0-9 '!?.]{2,40}")]
    text: String,
}
