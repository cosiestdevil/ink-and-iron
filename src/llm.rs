use std::ffi::CString;

use bevy::log::info;
use libloading::Library;
pub use llm_api::settlement_names::SettlementNameCtx;
use llm_api::{ByteStr, LLMOps, StatusCode, settlement_names::{ExternSettlementNameCtx}, unit_spawn_barks::{ExternUnitSpawnBarkCtx, UnitSpawnBarkCtx}};
use tokio::sync::{
    OnceCell,
    oneshot::{self, Sender},
};

pub async fn unit_spawn_barks(ctx:UnitSpawnBarkCtx,temp:f32) -> anyhow::Result<Vec<String>> {
    let ops = get_llm().await;
    let (tx, rx) = oneshot::channel();
    _unit_spawn_barks(ops, tx, ctx, temp);
    let res = rx.await?;
    Ok(res)
}

pub async fn settlement_names(ctx: SettlementNameCtx, temp: f32) -> anyhow::Result<Vec<String>> {
    let ops = get_llm().await;
    let (tx, rx) = oneshot::channel();
    _settlement_names(ops, tx, ctx, temp);
    let res = rx.await?;
    Ok(res)
}
fn _unit_spawn_barks(ops: &LLMOps, tx: Sender<Vec<String>>, ctx: UnitSpawnBarkCtx, temp: f32) {
    let cstr = CString::new(ctx.civilisation_name.clone()).expect("no interior NULs");
    let c_ptr = cstr.into_raw();
    let owned = Box::new(llm_api::unit_spawn_barks::OwnedCtx {
        tx,
        cstr_ptr: c_ptr,
        ctx: ExternUnitSpawnBarkCtx {
            civilisation_name: c_ptr,
            unit_type: CString::new(ctx.unit_type.clone()).expect("no interior NULs").into_raw(),
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
            let bytes = unsafe { core::slice::from_raw_parts(bs.ptr, bs.len) };
            if let Ok(s) = core::str::from_utf8(bytes) {
                // Own if needed:
                let owned: String = s.to_owned();
                names.push(owned);
            } else {
                // invalid utf-8; skip or record error
            }
        }
        let _ = owned.tx.send(names);
        if !owned.cstr_ptr.is_null() {
            unsafe {
                let _ = CString::from_raw(owned.cstr_ptr);
            } // drops and frees
        }
    }
    info!("Calling unit spawn barks");
    let a = ops.unit_spawn_barks;
    info!("Calling unit spawn barks");
    (a)(ctx_ptr, temp, user_data, unit_spawn_barks_callback);
    info!("Called unit spawn barks");
}
fn _settlement_names(ops: &LLMOps, tx: Sender<Vec<String>>, ctx: SettlementNameCtx, temp: f32) {
    let cstr = CString::new(ctx.civilisation_name.clone()).expect("no interior NULs");
    let c_ptr = cstr.into_raw();
    let owned = Box::new(llm_api::settlement_names::OwnedCtx {
        tx,
        cstr_ptr: c_ptr,
        ctx: ExternSettlementNameCtx {
            civilisation_name: c_ptr,
        },
    });
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
            let bytes = unsafe { core::slice::from_raw_parts(bs.ptr, bs.len) };
            if let Ok(s) = core::str::from_utf8(bytes) {
                // Own if needed:
                let owned: String = s.to_owned();
                names.push(owned);
            } else {
                // invalid utf-8; skip or record error
            }
        }
        let _ = owned.tx.send(names);
        if !owned.cstr_ptr.is_null() {
            unsafe {
                let _ = CString::from_raw(owned.cstr_ptr);
            } // drops and frees
        }
    }
    info!("Calling settlement names");
    let a = ops.settlement_names;
    info!("Calling settlement names");
    (a)(ctx_ptr, temp, user_data, settlement_names_callback);
    info!("Called settlement names");
}
static LLM: OnceCell<(LLMOps, Library)> = OnceCell::const_new();
pub async fn get_llm() -> &'static LLMOps {
    let a = LLM.get_or_init(async || load_llm().unwrap());
    &a.await.0
}

fn load_llm() -> anyhow::Result<(LLMOps, Library)> {
    unsafe {
        let mut lib = libloading::Library::new("llm_provider_cuda");
        if let Err(_err) = lib {
            info!("Cuda load failed, falling back to CPU LLM");
            lib = libloading::Library::new("llm_provider");
        }
        let lib = lib?;
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
            Ok((ops, lib))
        } else {
            panic!("AHH");
        }
    }
}
