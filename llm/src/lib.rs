use std::sync::Arc;

use libloading::Library;
pub use llm_api::settlement_names::SettlementNameCtx;
use llm_api::{
    ByteStr, LLMOps, SettlementNamesOutput, StatusCode, UnitSpawnBarksOutput,
    settlement_names::{self, ExternSettlementNameCtx},
    unit_spawn_barks::{ExternUnitSpawnBarkCtx, UnitSpawnBarkCtx},
};
use tokio::sync::{
    OnceCell, RwLock,
    oneshot::{self, Sender},
};
use tracing::info;
pub async fn unit_spawn_barks(
    llm_mode: Option<String>,
    ctx: UnitSpawnBarkCtx,
    temp: f32,
) -> anyhow::Result<Vec<String>> {
    let ops = get_llm(llm_mode).await;
    let (tx, rx) = oneshot::channel();
    let seed_barks = ctx.seed_barks.clone();
    info!(
        "Requesting unit spawn barks with seed barks: {:?}",
        seed_barks
    );
    _unit_spawn_barks(&ops.ops, tx, ctx, temp);
    let res = rx.await?;
    info!(
        "Received unit spawn barks: {:?} using seed barks: {:?}",
        res, seed_barks
    );
    Ok(res)
}

pub async fn settlement_names(
    llm_mode: Option<String>,
    ctx: SettlementNameCtx,
    temp: f32,
) -> anyhow::Result<Vec<String>> {
    let ops = get_llm(llm_mode).await;
    let (tx, rx) = oneshot::channel();
    let seed_names = ctx.seed_names.clone();
    info!(
        "Requesting settlement names with seed names: {:?}",
        seed_names
    );
    _settlement_names(&ops.ops, tx, ctx, temp);
    let res = rx.await?;
    info!(
        "Received settlement names: {:?} using seed name: {:?}",
        res, seed_names
    );
    Ok(res)
}
fn _unit_spawn_barks(ops: &LLMOps, tx: Sender<Vec<String>>, ctx: UnitSpawnBarkCtx, temp: f32) {
    let seed_barks = llm_api::as_bytestrs(&ctx.seed_barks);
    let owned = Box::new(llm_api::unit_spawn_barks::OwnedCtx {
        tx,
        ctx: ExternUnitSpawnBarkCtx {
            civilisation_name: ByteStr::from_string(&ctx.civilisation_name),
            civ_description: ByteStr::from_string(&ctx.civ_description),
            unit_type: ByteStr::from_string(&ctx.unit_type),
            seed_barks: seed_barks.as_ptr(),
            seed_barks_len: seed_barks.len(),
            description: ByteStr::from_string(&ctx.description),
        },
    });
    let ctx_ptr: *const ExternUnitSpawnBarkCtx = &owned.ctx;
    let user_data = Box::into_raw(owned);
    extern "C" fn unit_spawn_barks_callback(
        out_names: *const ByteStr,
        out_names_len: usize,
        user_data: *mut llm_api::unit_spawn_barks::OwnedCtx,
        _status: StatusCode,
    ) {
        let owned: Box<llm_api::unit_spawn_barks::OwnedCtx> = unsafe { Box::from_raw(user_data) };
        info!("Received unit spawn barks");
        let list = unsafe { core::slice::from_raw_parts(out_names, out_names_len) };
        let mut names = Vec::new();
        for bs in list {
            names.push(bs.as_string().clone());
        }
        let _ = owned.tx.send(names);
    }
    info!("Getting unit spawn barks function");
    let a = ops.unit_spawn_barks;
    info!("Calling unit spawn barks");
    (a)(ctx_ptr, temp, user_data, unit_spawn_barks_callback);
    info!("Called unit spawn barks");
}
fn _settlement_names(ops: &LLMOps, tx: Sender<Vec<String>>, ctx: SettlementNameCtx, temp: f32) {
    let seed_names = llm_api::as_bytestrs(&ctx.seed_names);
    let owned = Box::new(llm_api::settlement_names::OwnedCtx {
        tx,
        ctx: ExternSettlementNameCtx {
            civilisation_name: ByteStr::from_string(&ctx.civilisation_name),
            description: ByteStr::from_string(&ctx.description),
            seed_names: seed_names.as_ptr(),
            seed_names_len: ctx.seed_names.len(),
        },
    });
    println!("Created owned settlement names context: {:?}", owned);
    let ctx_ptr: *const ExternSettlementNameCtx = &owned.ctx;
    let user_data = Box::into_raw(owned);
    extern "C" fn settlement_names_callback(
        out_names: *const ByteStr,
        out_names_len: usize,
        user_data: *mut llm_api::settlement_names::OwnedCtx,
        _status: StatusCode,
    ) {
        let owned: Box<llm_api::settlement_names::OwnedCtx> = unsafe { Box::from_raw(user_data) };
        info!("Received settlement names");
        let list = unsafe { core::slice::from_raw_parts(out_names, out_names_len) };
        let mut names = Vec::new();
        for bs in list {
            names.push(bs.as_string().clone());
        }
        let _ = owned.tx.send(names);
    }
    info!("Getting settlement names function");
    let a = ops.settlement_names;
    info!("Calling settlement names");
    (a)(ctx_ptr, temp, user_data, settlement_names_callback);
    info!("Called settlement names");
}

pub struct LLMHandle {
    pub ops: LLMOps,
    pub _lib: Option<Library>,
}
impl LLMHandle {
    pub fn new(ops: LLMOps, lib: Option<Library>) -> Self {
        LLMHandle { ops, _lib: lib }
    }
}
static LLM: OnceCell<RwLock<Option<Arc<LLMHandle>>>> = OnceCell::const_new();
pub async fn get_llm(llm_mode: Option<String>) -> Arc<LLMHandle> {
    let llm_lock = LLM.get_or_init(|| async { RwLock::new(None) }).await;
    {
        let read_guard = llm_lock.read().await;
        if let Some(ref llm_arc) = *read_guard {
            return llm_arc.clone();
        }
    }
    let mut write_guard = llm_lock.write().await;
    if write_guard.is_none() {
        let (ops, lib) = load_llm(llm_mode).expect("Failed to load LLM");
        let llm_arc = Arc::new(LLMHandle::new(ops, lib));
        *write_guard = Some(llm_arc);
    }
    if let Some(ref llm_arc) = *write_guard {
        return llm_arc.clone();
    }
    unreachable!()
}

extern "C" fn no_llm_settlement_names(
    ctx: *const ExternSettlementNameCtx,
    _temp: f32,
    user: *mut settlement_names::OwnedCtx,
    done: SettlementNamesOutput,
) {
    let ctx_owned = unsafe { SettlementNameCtx::from_extern(ctx) };
    let user: Box<settlement_names::OwnedCtx> = unsafe { Box::from_raw(user) };
    let bytestrs: Vec<ByteStr> = ctx_owned
        .seed_names
        .iter()
        .map(|s| ByteStr::from_string(s))
        .collect();
    let user = Box::into_raw(user);
    done(bytestrs.as_ptr(), bytestrs.len(), user, StatusCode::OK);
}
extern "C" fn no_llm_unit_spawn_barks(
    ctx: *const ExternUnitSpawnBarkCtx,
    _temp: f32,
    user: *mut llm_api::unit_spawn_barks::OwnedCtx,
    done: UnitSpawnBarksOutput,
) {
    let ctx_owned = unsafe { UnitSpawnBarkCtx::from_extern(ctx) };
    let user: Box<llm_api::unit_spawn_barks::OwnedCtx> = unsafe { Box::from_raw(user) };
    let bytestrs: Vec<ByteStr> = ctx_owned
        .seed_barks
        .iter()
        .map(|s| ByteStr::from_string(s))
        .collect();
    let user = Box::into_raw(user);
    done(bytestrs.as_ptr(), bytestrs.len(), user, StatusCode::OK);
}
fn load_llm(llm_mode: Option<String>) -> anyhow::Result<(LLMOps, Option<Library>)> {
    match llm_mode {
        Some(path) => load_llm_internal(path),
        None => {
            let ops = LLMOps {
                settlement_names: no_llm_settlement_names,
                unit_spawn_barks: no_llm_unit_spawn_barks,
            };
            Ok((ops, None))
        }
    }
}
fn load_llm_internal(path: String) -> anyhow::Result<(LLMOps, Option<Library>)> {
    unsafe {
        let lib = libloading::Library::new(path)?;
        info!("Found Library");
        let func: libloading::Symbol<llm_api::CreateFn> = lib.get(b"create_llm_provider")?;
        info!("Found Create Function");
        use core::mem::MaybeUninit;

        // 1) Create uninitialized storage
        let mut out = MaybeUninit::<LLMOps>::uninit();

        let ok = func(out.as_mut_ptr());
        if ok {
            let ops = out.assume_init();
            info!("Loaded Ops");
            Ok((ops, Some(lib)))
        } else {
            panic!("AHH");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_no_llm_settlement_names() {
        *LLM.get_or_init(|| async { RwLock::new(None) })
            .await
            .write()
            .await = None;
        let expected = vec!["Alpha".to_string(), "Beta".to_string()];
        let names = settlement_names(
            None,
            SettlementNameCtx {
                civilisation_name: "TestCiv".to_string(),
                description: "Test description".to_string(),
                seed_names: vec!["Alpha".to_string(), "Beta".to_string()],
            },
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(names, expected);
    }
    
}
