//! Direct `root\WMI:AcerGamingFunction` access through COM.
//!
//! COM interfaces are owned by one dedicated MTA worker thread; callers send
//! requests over a channel, which keeps [`WmiTransport`] `Send + Sync`.

use std::thread;

use crossbeam_channel::{bounded, Sender};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use opensense_proto::wmi::{Shape, WMI_CLASS, WMI_NAMESPACE};
use opensense_proto::GamingMethod;
use windows::core::{BSTR, PCWSTR, VARIANT};
use windows::Win32::Foundation::RPC_E_TOO_LATE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
    SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayCreateVector, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{
    VT_ARRAY, VT_BSTR, VT_I1, VT_I2, VT_I4, VT_I8, VT_INT, VT_UI1, VT_UI2, VT_UI4, VT_UI8, VT_UINT,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_GENERIC_FLAG_TYPE,
};

use crate::{GamingTransport, HwError, WmiValue};

const WBEM_E_ACCESS_DENIED: i32 = 0x8004_1003_u32 as i32;
const WBEM_E_INVALID_CLASS: i32 = 0x8004_1010_u32 as i32;
const WBEM_E_NOT_FOUND: i32 = 0x8004_1002_u32 as i32;

enum Input {
    U64(u64),
    U32(u32),
    Bytes(Vec<u8>),
    None,
}

enum Output {
    U64(u64),
    U32(u32),
    Bytes(Vec<u8>),
}

/// A queued call. `deadline` is when its caller stops waiting: a request
/// still queued by then (behind a hung call) is dropped rather than run
/// late, so a stale fan or mode write cannot land on top of newer state.
enum Request {
    Gaming {
        method: GamingMethod,
        input: Input,
        deadline: std::time::Instant,
        reply: Sender<Result<Output, HwError>>,
    },
    Raw {
        deadline: std::time::Instant,
        class: &'static str,
        method: &'static str,
        inputs: Vec<(&'static str, WmiValue)>,
        outputs: Vec<&'static str>,
        reply: Sender<Result<Vec<WmiValue>, HwError>>,
    },
}

pub struct WmiTransport {
    tx: Sender<Request>,
}

impl WmiTransport {
    /// Spawns the COM worker and verifies that the class is reachable.
    pub fn connect() -> Result<Self, HwError> {
        let (tx, rx) = bounded::<Request>(16);
        let (ready_tx, ready_rx) = bounded::<Result<(), HwError>>(1);
        thread::Builder::new()
            .name("opensense-wmi".into())
            .spawn(move || {
                if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
                    let _ = ready_tx.send(Err(map_err(e)));
                    return;
                }
                let session = match unsafe { Session::open() } {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        unsafe { CoUninitialize() };
                        return;
                    }
                };
                for req in rx {
                    let expired = |d: std::time::Instant| std::time::Instant::now() >= d;
                    match req {
                        Request::Gaming { deadline, .. } | Request::Raw { deadline, .. } if expired(deadline) => {
                            tracing::warn!("dropped a firmware call that waited too long");
                        }
                        Request::Gaming { method, input, reply, .. } => {
                            let _ = reply.send(unsafe { session.call(method, input) });
                        }
                        Request::Raw { class, method, inputs, outputs, reply, .. } => {
                            let _ = reply.send(unsafe { session.call_raw(class, method, &inputs, &outputs) });
                        }
                    }
                }
                drop(session);
                unsafe { CoUninitialize() };
            })
            .map_err(|e| HwError::Wmi(e.to_string()))?;
        // Bounded: connecting to a stuck WMI service must not hang the caller.
        match ready_rx.recv_timeout(CONNECT_TIMEOUT) {
            Ok(r) => r?,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                return Err(HwError::Wmi("connecting to WMI timed out".into()))
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(HwError::Wmi("WMI worker exited".into()))
            }
        }
        Ok(Self { tx })
    }

    fn call(&self, method: GamingMethod, input: Input) -> Result<Output, HwError> {
        let (reply, rx) = bounded(1);
        self.submit(Request::Gaming { method, input, deadline: deadline(), reply })?;
        wait_reply(&rx)
    }

    /// Queues a request; fails instead of blocking when the queue is full
    /// (the worker is stuck in a call).
    fn submit(&self, req: Request) -> Result<(), HwError> {
        self.tx.send_timeout(req, CALL_TIMEOUT).map_err(|e| match e {
            crossbeam_channel::SendTimeoutError::Timeout(_) => HwError::Wmi("firmware call timed out".into()),
            crossbeam_channel::SendTimeoutError::Disconnected(_) => HwError::Wmi("WMI worker exited".into()),
        })
    }
}

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn deadline() -> std::time::Instant {
    std::time::Instant::now() + CALL_TIMEOUT
}

/// Longest a firmware call may take. A call that hangs (ACPI stuck after
/// resume, WMI service restarting) then fails instead of blocking the
/// caller forever; the caller reconnects, which starts a fresh WMI thread.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn wait_reply<T>(rx: &crossbeam_channel::Receiver<Result<T, HwError>>) -> Result<T, HwError> {
    match rx.recv_timeout(CALL_TIMEOUT) {
        Ok(r) => r,
        Err(crossbeam_channel::RecvTimeoutError::Timeout) => Err(HwError::Wmi("firmware call timed out".into())),
        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => Err(HwError::Wmi("WMI worker exited".into())),
    }
}

impl GamingTransport for WmiTransport {
    fn name(&self) -> &'static str {
        "acpi-wmi"
    }

    fn set_u64(&self, method: GamingMethod, input: u64) -> Result<u32, HwError> {
        expect_shape(method, Shape::SetU64)?;
        match self.call(method, Input::U64(input))? {
            Output::U32(v) => Ok(v),
            Output::U64(v) => Ok(v as u32),
            Output::Bytes(_) => Err(HwError::Shape(method.wmi_name())),
        }
    }

    fn get_u32(&self, method: GamingMethod, input: u32) -> Result<u64, HwError> {
        let input = match method.shape() {
            Shape::GetU32 => Input::U32(input),
            Shape::GetNoInput => Input::None,
            _ => return Err(HwError::Shape(method.wmi_name())),
        };
        match self.call(method, input)? {
            Output::U64(v) => Ok(v),
            Output::U32(v) => Ok(u64::from(v)),
            Output::Bytes(_) => Err(HwError::Shape(method.wmi_name())),
        }
    }

    fn set_bytes(&self, method: GamingMethod, input: &[u8]) -> Result<u32, HwError> {
        expect_shape(method, Shape::SetBytes)?;
        match self.call(method, Input::Bytes(input.to_vec()))? {
            Output::U32(v) => Ok(v),
            Output::U64(v) => Ok(v as u32),
            Output::Bytes(_) => Err(HwError::Shape(method.wmi_name())),
        }
    }

    fn get_bytes(&self, method: GamingMethod, input: u32) -> Result<Vec<u8>, HwError> {
        expect_shape(method, Shape::GetBytes)?;
        match self.call(method, Input::U32(input))? {
            Output::Bytes(b) => Ok(b),
            _ => Err(HwError::Shape(method.wmi_name())),
        }
    }

    fn call_raw(
        &self,
        class: &'static str,
        method: &'static str,
        inputs: &[(&'static str, WmiValue)],
        outputs: &[&'static str],
    ) -> Result<Vec<WmiValue>, HwError> {
        let (reply, rx) = bounded(1);
        self.submit(Request::Raw {
            deadline: deadline(),
            class,
            method,
            inputs: inputs.to_vec(),
            outputs: outputs.to_vec(),
            reply,
        })?;
        wait_reply(&rx)
    }
}

fn expect_shape(method: GamingMethod, shape: Shape) -> Result<(), HwError> {
    if method.shape() == shape {
        Ok(())
    } else {
        Err(HwError::Shape(method.wmi_name()))
    }
}

fn map_err(e: windows::core::Error) -> HwError {
    match e.code().0 {
        WBEM_E_ACCESS_DENIED => HwError::AccessDenied,
        WBEM_E_INVALID_CLASS | WBEM_E_NOT_FOUND => HwError::NotPresent,
        _ => HwError::Wmi(format!("{e} ({:#010x})", e.code().0)),
    }
}

/// Class definition plus the path of its (single) instance.
struct ClassHandle {
    class: IWbemClassObject,
    instance_path: BSTR,
}

/// COM state living on the worker thread.
struct Session {
    services: IWbemServices,
    classes: RefCell<HashMap<&'static str, Rc<ClassHandle>>>,
    /// Input-parameter signatures per (class, method); `GetMethod` is the
    /// expensive part of a call, so it runs once per method.
    signatures: RefCell<HashMap<(&'static str, &'static str), Option<IWbemClassObject>>>,
}

impl Session {
    /// Requires COM to be initialised (MTA) on the calling thread.
    unsafe fn open() -> Result<Self, HwError> {
        if let Err(e) = CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        ) {
            if e.code() != RPC_E_TOO_LATE {
                return Err(map_err(e));
            }
        }
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(map_err)?;
        let services = locator
            .ConnectServer(&BSTR::from(WMI_NAMESPACE), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None)
            .map_err(map_err)?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
        .map_err(map_err)?;

        let session = Self { services, classes: RefCell::default(), signatures: RefCell::default() };
        // Resolve the main class now so access/presence errors surface at connect.
        session.class(WMI_CLASS)?;
        Ok(session)
    }

    unsafe fn class(&self, name: &'static str) -> Result<Rc<ClassHandle>, HwError> {
        if let Some(h) = self.classes.borrow().get(name) {
            return Ok(h.clone());
        }
        let services = &self.services;
        let mut class = None;
        services
            .GetObject(&BSTR::from(name), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)
            .map_err(map_err)?;
        let class = class.ok_or(HwError::NotPresent)?;

        let query = format!("SELECT * FROM {name}");
        let enumerator = services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query.as_str()),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .map_err(map_err)?;
        let mut objs = [None];
        let mut returned = 0u32;
        // 10 s at most; a timeout returns no object (NotPresent below).
        enumerator.Next(10_000, &mut objs, &mut returned).ok().map_err(map_err)?;
        let instance = match (returned, objs[0].take()) {
            (1, Some(o)) => o,
            _ => return Err(HwError::NotPresent),
        };
        let mut path = VARIANT::default();
        instance.Get(PCWSTR::from_raw(wide("__PATH").as_ptr()), 0, &mut path, None, None).map_err(map_err)?;
        let instance_path = BSTR::try_from(&path).map_err(map_err)?;
        tracing::debug!(path = %instance_path, "{name} instance");
        let handle = Rc::new(ClassHandle { class, instance_path });
        self.classes.borrow_mut().insert(name, handle.clone());
        Ok(handle)
    }

    unsafe fn signature(
        &self,
        handle: &ClassHandle,
        class: &'static str,
        method: &'static str,
        wide_name: &[u16],
    ) -> Result<Option<IWbemClassObject>, HwError> {
        if let Some(sig) = self.signatures.borrow().get(&(class, method)) {
            return Ok(sig.clone());
        }
        let mut in_sig = None;
        handle
            .class
            .GetMethod(PCWSTR::from_raw(wide_name.as_ptr()), 0, &mut in_sig, std::ptr::null_mut())
            .map_err(map_err)?;
        self.signatures.borrow_mut().insert((class, method), in_sig.clone());
        Ok(in_sig)
    }

    unsafe fn call(&self, method: GamingMethod, input: Input) -> Result<Output, HwError> {
        let handle = self.class(method.wmi_class())?;
        let (in_name, out_name) = method.param_names();
        let name = wide(method.wmi_name());
        let in_sig = self.signature(&handle, method.wmi_class(), method.wmi_name(), &name)?;

        let in_params = match (&input, in_sig) {
            (Input::None, _) => None,
            (_, None) => return Err(HwError::Shape(method.wmi_name())),
            (input, Some(sig)) => {
                let params = sig.SpawnInstance(0).map_err(map_err)?;
                let value = match input {
                    // CIM uint64 travels as a decimal BSTR.
                    Input::U64(v) => VARIANT::from(BSTR::from(v.to_string().as_str())),
                    Input::U32(v) => VARIANT::from(*v as i32),
                    Input::Bytes(b) => byte_array_variant(b)?,
                    Input::None => unreachable!(),
                };
                params.Put(PCWSTR::from_raw(wide(in_name).as_ptr()), 0, &value, 0).map_err(map_err)?;
                Some(params)
            }
        };

        let mut out = None;
        self.services
            .ExecMethod(
                &handle.instance_path,
                &BSTR::from(method.wmi_name()),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                in_params.as_ref(),
                Some(&mut out),
                None,
            )
            .map_err(map_err)?;
        let out = out.ok_or_else(|| HwError::Wmi(format!("{} returned no output", method.wmi_name())))?;

        let mut value = VARIANT::default();
        out.Get(PCWSTR::from_raw(wide(out_name).as_ptr()), 0, &mut value, None, None).map_err(map_err)?;
        let res = match method.shape() {
            Shape::GetU32 | Shape::GetNoInput => Output::U64(variant_u64(&value)?),
            Shape::SetU64 | Shape::SetBytes => Output::U32(variant_u64(&value)? as u32),
            Shape::GetBytes => Output::Bytes(variant_bytes(&value)?),
        };
        tracing::trace!(method = method.wmi_name(), "WMI call ok");
        Ok(res)
    }
}

impl Session {
    /// Generic call with named parameters (integers travel as VT_I4 and are
    /// coerced by WMI to the declared CIM type).
    unsafe fn call_raw(
        &self,
        class: &'static str,
        method: &'static str,
        inputs: &[(&'static str, WmiValue)],
        outputs: &[&'static str],
    ) -> Result<Vec<WmiValue>, HwError> {
        let handle = self.class(class)?;
        let name = wide(method);
        let in_sig = self.signature(&handle, class, method, &name)?;
        let in_params = match in_sig {
            Some(sig) => {
                let params = sig.SpawnInstance(0).map_err(map_err)?;
                for (pname, value) in inputs {
                    let v = match value {
                        WmiValue::Int(i) => VARIANT::from(*i as i32),
                        WmiValue::Bytes(b) => byte_array_variant(b)?,
                    };
                    params.Put(PCWSTR::from_raw(wide(pname).as_ptr()), 0, &v, 0).map_err(map_err)?;
                }
                Some(params)
            }
            None => None,
        };
        let mut out = None;
        self.services
            .ExecMethod(
                &handle.instance_path,
                &BSTR::from(method),
                WBEM_GENERIC_FLAG_TYPE(0),
                None,
                in_params.as_ref(),
                Some(&mut out),
                None,
            )
            .map_err(map_err)?;
        let out = out.ok_or_else(|| HwError::Wmi(format!("{method} returned no output")))?;
        let mut result = Vec::with_capacity(outputs.len());
        for oname in outputs {
            let mut v = VARIANT::default();
            out.Get(PCWSTR::from_raw(wide(oname).as_ptr()), 0, &mut v, None, None).map_err(map_err)?;
            let vt = v.as_raw().Anonymous.Anonymous.vt;
            result.push(if vt & VT_ARRAY.0 != 0 {
                WmiValue::Bytes(variant_bytes(&v)?)
            } else {
                WmiValue::Int(variant_u64(&v)? as u32)
            });
        }
        Ok(result)
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads an unsigned integer by its exact VARIANT type. WMI returns CIM
/// uint64 as a decimal BSTR and uint32 as VT_I4, whose bit pattern is kept
/// (a value of 0x8000_0000 or more must not be read as negative).
fn variant_u64(v: &VARIANT) -> Result<u64, HwError> {
    let raw = v.as_raw();
    let vt = unsafe { raw.Anonymous.Anonymous.vt };
    let val = unsafe { &raw.Anonymous.Anonymous.Anonymous };
    let n = match vt {
        x if x == VT_BSTR.0 => {
            let b = BSTR::try_from(v).map_err(map_err)?;
            return b.to_string().trim().parse::<u64>().map_err(|e| HwError::Wmi(format!("bad uint64 '{b}': {e}")));
        }
        x if x == VT_UI1.0 => u64::from(unsafe { val.bVal }),
        x if x == VT_I1.0 => u64::from((unsafe { val.cVal }) as u8),
        x if x == VT_UI2.0 => u64::from(unsafe { val.uiVal }),
        x if x == VT_I2.0 => u64::from((unsafe { val.iVal }) as u16),
        x if x == VT_UI4.0 || x == VT_UINT.0 => u64::from(unsafe { val.ulVal }),
        x if x == VT_I4.0 || x == VT_INT.0 => u64::from((unsafe { val.lVal }) as u32),
        x if x == VT_UI8.0 => unsafe { val.ullVal },
        x if x == VT_I8.0 => (unsafe { val.llVal }) as u64,
        _ => return Err(HwError::Wmi(format!("unexpected VARIANT type {vt:#x}"))),
    };
    Ok(n)
}

unsafe fn byte_array_variant(bytes: &[u8]) -> Result<VARIANT, HwError> {
    let sa: *mut SAFEARRAY = SafeArrayCreateVector(VT_UI1, 0, bytes.len() as u32);
    if sa.is_null() {
        return Err(HwError::Wmi("SafeArrayCreateVector failed".into()));
    }
    let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Err(e) = SafeArrayAccessData(sa, &mut data) {
        let _ = SafeArrayDestroy(sa);
        return Err(map_err(e));
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), bytes.len());
    if let Err(e) = SafeArrayUnaccessData(sa) {
        let _ = SafeArrayDestroy(sa);
        return Err(map_err(e));
    }
    // The VARIANT takes ownership of the array and destroys it on drop.
    let mut raw: windows_core::imp::VARIANT = std::mem::zeroed();
    raw.Anonymous.Anonymous.vt = VT_ARRAY.0 | VT_UI1.0;
    raw.Anonymous.Anonymous.Anonymous.parray = sa.cast();
    Ok(VARIANT::from_raw(raw))
}

unsafe fn variant_bytes(v: &VARIANT) -> Result<Vec<u8>, HwError> {
    let raw = v.as_raw();
    let vt = raw.Anonymous.Anonymous.vt;
    if vt != (VT_ARRAY.0 | VT_UI1.0) {
        return Err(HwError::Wmi(format!("expected byte array, got vt={vt:#x}")));
    }
    let sa = raw.Anonymous.Anonymous.Anonymous.parray as *const SAFEARRAY;
    let lo = SafeArrayGetLBound(sa, 1).map_err(map_err)?;
    let hi = SafeArrayGetUBound(sa, 1).map_err(map_err)?;
    let len = (hi - lo + 1).max(0) as usize;
    if len == 0 {
        return Ok(Vec::new());
    }
    let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
    SafeArrayAccessData(sa, &mut data).map_err(map_err)?;
    let out = std::slice::from_raw_parts(data.cast::<u8>(), len).to_vec();
    SafeArrayUnaccessData(sa).map_err(map_err)?;
    Ok(out)
}
