
#[repr(C)]
pub struct LLMOps {
    pub settlement_names: extern "C" fn(
        ctx: *const settlement_names::ExternSettlementNameCtx,
        temp: f32,
        tx: *mut settlement_names::OwnedCtx,
        done: SettlementNamesOutput,
    ),
    pub unit_spawn_barks: extern "C" fn(
        ctx: *const unit_spawn_barks::ExternUnitSpawnBarkCtx,
        temp: f32,
        tx: *mut unit_spawn_barks::OwnedCtx,
        done: UnitSpawnBarksOutput,
    ),
}
pub type CreateFn = extern "C" fn(out_ops: *mut LLMOps) -> bool;
#[repr(C)]
pub struct ByteStr {
    pub ptr: *const u8,
    pub len: usize,
}
pub type UnitSpawnBarksOutput = extern "C" fn(
    out_names: *const ByteStr,
    out_names_len: usize,
    tx: *mut unit_spawn_barks::OwnedCtx,
    status: StatusCode,
);
pub type SettlementNamesOutput = extern "C" fn(
    out_names: *const ByteStr,
    out_names_len: usize,
    tx: *mut settlement_names::OwnedCtx,
    status: StatusCode,
);

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StatusCode {
    OK = 0,
    Error = 1,
}
pub mod settlement_names {
    use std::ffi::c_char;

    use serde::{Deserialize, Serialize};
    use tokio::sync::oneshot;

    use crate::ByteStr;
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
            SettlementNameCtx {
                civilisation_name: c.to_string_lossy().into_owned(),
            }
        }
    }

    pub fn as_bytestrs(strings: &[String]) -> Vec<ByteStr> {
        strings
            .iter()
            .map(|s| ByteStr {
                ptr: s.as_ptr(),
                len: s.len(),
            })
            .collect()
    }
    pub struct OwnedCtx {
        pub tx: oneshot::Sender<Vec<String>>,
        pub cstr_ptr: *mut c_char,        // to free later
        pub ctx: ExternSettlementNameCtx, // lives on heap via this Box
    }
    unsafe impl Send for OwnedCtx {}
}
pub mod unit_spawn_barks {
    use std::ffi::c_char;

    use serde::{Deserialize, Serialize};
    use tokio::sync::oneshot;

    use crate::ByteStr;
    #[repr(C)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct ExternUnitSpawnBarkCtx {
        pub civilisation_name: *mut c_char,
        pub unit_type:  *mut c_char,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct UnitSpawnBarkCtx {
        pub civilisation_name: String,
        pub unit_type: String,
    }
    impl UnitSpawnBarkCtx {
        /// .
        ///
        /// # Safety
        ///
        /// .
        pub unsafe fn from_extern(p: *const ExternUnitSpawnBarkCtx) -> Self {
            assert!(!p.is_null());
            
            UnitSpawnBarkCtx {
                civilisation_name: {
                    let c = unsafe { std::ffi::CStr::from_ptr((*p).civilisation_name) };
                    c.to_string_lossy().into_owned()
                },
                unit_type: {
                    let c = unsafe { std::ffi::CStr::from_ptr((*p).unit_type) };
                    c.to_string_lossy().into_owned()
                }
            }
        }
    }

    pub fn as_bytestrs(strings: &[String]) -> Vec<ByteStr> {
        strings
            .iter()
            .map(|s| ByteStr {
                ptr: s.as_ptr(),
                len: s.len(),
            })
            .collect()
    }
    pub struct OwnedCtx {
        pub tx: oneshot::Sender<Vec<String>>,
        pub cstr_ptr: *mut c_char,        // to free later
        pub ctx: ExternUnitSpawnBarkCtx, // lives on heap via this Box
    }
    unsafe impl Send for OwnedCtx {}
}