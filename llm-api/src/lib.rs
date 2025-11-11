use std::ffi::{c_char};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[repr(C)]
pub struct LLMOps {
    pub settlement_names:
        extern "C" fn(ctx: *const ExternSettlementNameCtx, temp: f32,tx:*mut OwnedCtx, done: SettlementNamesOutput),
}
pub type CreateFn = extern "C" fn(out_ops: *mut LLMOps) -> bool;
#[repr(C)]
pub struct ByteStr {
    pub ptr: *const u8,
    pub len: usize,
}
pub type SettlementNamesOutput =
    extern "C" fn(out_names: *const ByteStr, out_names_len: usize,tx:*mut OwnedCtx, status: StatusCode);

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StatusCode {
    OK = 0,
    Error = 1,
}
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ExternSettlementNameCtx {
    pub civilisation_name: *mut c_char,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettlementNameCtx {
    pub civilisation_name: String,
}
impl SettlementNameCtx {
    /// .
    ///
    /// # Safety
    ///
    /// .
    pub unsafe fn from_extern(p: *const ExternSettlementNameCtx) -> Self {
        assert!(!p.is_null());
        let c = unsafe { std::ffi::CStr::from_ptr((*p).civilisation_name) };
        SettlementNameCtx { civilisation_name: c.to_string_lossy().into_owned() }
    }
}

pub fn as_bytestrs(strings: &[String]) -> Vec<ByteStr> {
    strings
        .iter()
        .map(|s| ByteStr { ptr: s.as_ptr(), len: s.len() })
        .collect()
}
pub struct OwnedCtx {
    pub tx: oneshot::Sender<Vec<String>>,
    pub cstr_ptr: *mut c_char,        // to free later
    pub ctx: ExternSettlementNameCtx, // lives on heap via this Box
}
unsafe impl Send for OwnedCtx{}