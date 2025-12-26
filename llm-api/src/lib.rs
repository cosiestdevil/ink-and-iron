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
#[derive(Debug,Copy,Clone,PartialEq,Eq)]
pub struct ByteStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl ByteStr {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
    pub fn as_string(&self) -> String {
        String::from_utf8_lossy(self.as_slice()).to_string()
    }
    pub fn from_string(s: &str) -> Self {
        ByteStr {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }
}
pub fn as_bytestrs(strings: &[String]) -> Vec<ByteStr> {
        strings
            .iter()
            .map(|s| {
                println!("Converting string to ByteStr: ptr={:?}, len={}", s.as_ptr(),s.len());
                ByteStr::from_string(s)
            })
            .collect()
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

    use serde::{Deserialize, Serialize};
    use tokio::sync::oneshot;

    use crate::ByteStr;
    #[repr(C)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct ExternSettlementNameCtx {
        pub civilisation_name: ByteStr,
        pub seed_names: *const ByteStr,
        pub seed_names_len: usize,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SettlementNameCtx {
        pub civilisation_name: String,
        pub seed_names: Vec<String>,
    }
    impl SettlementNameCtx {
        /// .
        ///
        /// # Safety
        ///
        /// .
        pub unsafe fn from_extern(p: *const ExternSettlementNameCtx) -> Self {
            assert!(!p.is_null());
            let t = unsafe{(*p).civilisation_name};
            println!("ctx: {:?}", unsafe{*p} );
            let seeds_slice = unsafe { std::slice::from_raw_parts(
                (*p).seed_names,
                (*p).seed_names_len,
            ) };
            SettlementNameCtx {
                civilisation_name:t.as_string(),
                seed_names: seeds_slice
                    .iter()
                    .map(|bs| {
                        println!("seed name ByteStr: ptr={:?}, len={}", bs.ptr, bs.len);
                        bs.as_string()
                    })
                    .collect(),
            }
        }
    }

    
    #[derive(Debug)]
    pub struct OwnedCtx {
        pub tx: oneshot::Sender<Vec<String>>,     // to free later
        pub ctx: ExternSettlementNameCtx, // lives on heap via this Box
    }
    unsafe impl Send for OwnedCtx {}
}
pub mod unit_spawn_barks {

    use serde::{Deserialize, Serialize};
    use tokio::sync::oneshot;

    use crate::ByteStr;
    #[repr(C)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct ExternUnitSpawnBarkCtx {
        pub civilisation_name: ByteStr,
        pub unit_type:  ByteStr,
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
                civilisation_name: 
                    unsafe{(*p).civilisation_name.as_string()}
                ,
                unit_type: {
                    unsafe{(*p).unit_type.as_string()}
                }
            }
        }
    }

    pub struct OwnedCtx {
        pub tx: oneshot::Sender<Vec<String>>,      // to free later
        pub ctx: ExternUnitSpawnBarkCtx, // lives on heap via this Box
    }
    unsafe impl Send for OwnedCtx {}
}