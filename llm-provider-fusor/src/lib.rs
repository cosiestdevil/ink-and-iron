use kalosm_llama::FileSource;
use kalosm_llama::Llama;
use kalosm_llama::LlamaSource;
use kalosm_llama::prelude::ChatModelExt;
use kalosm_llama::prelude::GenerationParameters;
use kalosm_sample::Parse;
use kalosm_sample::Schema;
use llm_api::{
    ByteStr, LLMOps, SettlementNamesOutput, StatusCode,
    settlement_names::{self, ExternSettlementNameCtx, SettlementNameCtx},
    unit_spawn_barks::UnitSpawnBarkCtx,
};
use log::info;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

/// .
///
/// # Safety
///
/// .
#[unsafe(no_mangle)]
pub unsafe extern "C" fn create_llm_provider(out_ops: *mut LLMOps) -> bool {
    if out_ops.is_null() {
        return false;
    }
    unsafe {
        *out_ops = LLMOps {
            settlement_names: extern_settlement_names,
            unit_spawn_barks: extern_unit_spawn_barks,
        }
    };
    true
}
static RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();
fn rt_handle() -> tokio::runtime::Handle {
    if let Ok(h) = tokio::runtime::Handle::try_current() {
        h // reuse caller’s when present
    } else {
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("make runtime")
        })
        .handle()
        .clone()
    }
}
extern "C" fn extern_settlement_names(
    ctx: *const ExternSettlementNameCtx,
    temp: f32,
    user: *mut settlement_names::OwnedCtx,
    done: SettlementNamesOutput,
) {
    eprintln!("Starting Settlment names");
    let ctx_owned = unsafe {
        SettlementNameCtx::from_extern(ctx)
        // let name = std::ffi::CStr::from_ptr((*ctx).civilisation_name)
        //     .to_string_lossy()
        //     .into_owned();
        // SettlementNameCtx {
        //     civilisation_name: name,
        // }
    };
    println!("Munged context");

    let user: Box<settlement_names::OwnedCtx> = unsafe { Box::from_raw(user) };
    rt_handle().spawn(async move {
        match settlement_names(ctx_owned, temp).await {
            Ok(names_vec) => {
                println!("Have Settlement names");
                let bytestrs: Vec<ByteStr> =
                    names_vec.iter().map(|s| ByteStr::from_string(s)).collect();
                println!("munged Settlement names");
                let user = Box::into_raw(user);
                done(bytestrs.as_ptr(), bytestrs.len(), user, StatusCode::OK);
            }
            Err(_) => {
                let empty: [ByteStr; 0] = [];
                let user = Box::into_raw(user);
                done(empty.as_ptr(), 0, user, StatusCode::Error);
            }
        }
    });
}

extern "C" fn extern_unit_spawn_barks(
    ctx: *const llm_api::unit_spawn_barks::ExternUnitSpawnBarkCtx,
    temp: f32,
    user: *mut llm_api::unit_spawn_barks::OwnedCtx,
    done: llm_api::UnitSpawnBarksOutput,
) {
    let ctx_owned = unsafe { llm_api::unit_spawn_barks::UnitSpawnBarkCtx::from_extern(ctx) };
    println!("Munged context");

    let user: Box<llm_api::unit_spawn_barks::OwnedCtx> = unsafe { Box::from_raw(user) };
    rt_handle().spawn(async move {
        match unit_spawn_barks(ctx_owned, temp).await {
            Ok(barks_vec) => {
                println!("Have Unit Spawn Barks");
                let bytestrs: Vec<ByteStr> =
                    barks_vec.iter().map(|s| ByteStr::from_string(s)).collect();
                println!("munged Unit Spawn Barks");
                let user = Box::into_raw(user);
                done(bytestrs.as_ptr(), bytestrs.len(), user, StatusCode::OK);
            }
            Err(_) => {
                let empty: [ByteStr; 0] = [];
                let user = Box::into_raw(user);
                done(empty.as_ptr(), 0, user, StatusCode::Error);
            }
        }
    });
}
static LLM: tokio::sync::OnceCell<Llama> = tokio::sync::OnceCell::const_new();
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
    })
    .await
}
pub async fn unit_spawn_barks(ctx: UnitSpawnBarkCtx, temp: f32) -> anyhow::Result<Vec<String>> {
    let params = GenerationParameters::default()
        .with_max_length(32) // ~enough for short barks
        .with_temperature(temp)
        .with_top_p(0.9)
        .with_repetition_penalty(1.12);
    let llm = get_llm().await;
    let prompt = r#"You output ONLY a single JSON object that conforms EXACTLY to the provided JSON Schema.
Absolutely no extra text, no explanations, no examples, no code fences.

Content rules for each bark:
- 2-6 words.
- In theme with the Civilisation Name
- In theme with the Unit Type
- Informed by the Civilisation Description
- Informed by the provided seed barks.
- Informed by the Unit Description.
- Only these characters: letters, numbers, spaces, . ! ? and (optionally) '.
- Do not echo template tokens or placeholders (e.g., @handle, #Tag, %TOKEN%).
- Must be UTF-8 compliant.

If any bark would violate the rules, replace it with a different bark that complies.
"#;
    let task = llm.task(prompt).typed::<UnitSpawnBarkPack>();
    let stream = task
        .run(serde_json::ser::to_string(&ctx)?)
        .with_sampler(params.clone());
    let names = stream.await.unwrap();
    Ok(names
        .unit_spawn_barks
        .iter()
        .map(|n| n.text.clone())
        .collect())
}
pub async fn settlement_names(ctx: SettlementNameCtx, temp: f32) -> anyhow::Result<Vec<String>> {
    let params = GenerationParameters::default()
        .with_max_length(48) // ~enough for 3 short lines
        .with_temperature(temp)
        .with_top_p(0.9)
        .with_repetition_penalty(1.12);
    let llm = get_llm().await;
    let prompt = r#"You output ONLY a single JSON object that conforms EXACTLY to the provided JSON Schema.
Absolutely no extra text, no explanations, no examples, no code fences.

Content rules for each name:
- 1-3 words.
- In theme with the Civilisation Name
- Based on the provided Seed Names
- Informed by the Civilisation Description
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
    println!("Names Generated: {:?}", names.settlement_names);
    Ok(names
        .settlement_names
        .iter()
        .map(|n| n.text.clone())
        .collect())
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

#[derive(Parse, Schema, Clone, Debug, Serialize, Deserialize)]
struct UnitSpawnBarkPack {
    unit_spawn_barks: [UnitSpawnBark; 10],
}
#[derive(Parse, Schema, Clone, Debug, Serialize, Deserialize)]
struct UnitSpawnBark {
    // You can tighten this regex to your exact rules
    #[parse(pattern = r"[A-Za-z0-9 '!?.]{2,40}")]
    text: String,
}
