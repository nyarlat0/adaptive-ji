//! Exercise the actual VST3 component's state stream, not just EngineSettings serde.
use super::*;
use logic_nih_plug::wrapper::{
    state::PluginState,
    vst3::{Wrapper, vst3_sys as vst3_com},
};
use std::{
    collections::BTreeMap,
    ffi::c_void,
    io::{Cursor, Read, Seek, SeekFrom, Write},
};
use vst3_com::{
    ComInterface, VST3,
    base::{IBStream, IUnknown, kResultFalse, kResultOk, tresult},
    utils::SharedVstPtr,
    vst::{IAudioProcessor, IComponent, ProcessSetup},
};

#[VST3(implements(IBStream))]
struct StateStream {
    bytes: RefCell<Cursor<Vec<u8>>>,
}

impl StateStream {
    fn new(bytes: Vec<u8>) -> Box<Self> {
        Self::allocate(RefCell::new(Cursor::new(bytes)))
    }
    fn shared(&self) -> SharedVstPtr<dyn IBStream> {
        let mut raw = std::ptr::null_mut();
        // The returned borrow stays valid while this boxed test stream lives.
        // SharedVstPtr is repr(transparent) over the queried interface pointer.
        unsafe {
            assert_eq!(
                self.query_interface(&<dyn IBStream>::IID, &mut raw),
                kResultOk
            );
            let ptr = std::mem::transmute::<*mut c_void, SharedVstPtr<dyn IBStream>>(raw);
            self.release(); // Balance query_interface; this method returns a borrow.
            ptr
        }
    }
    fn state(&self) -> PluginState {
        serde_json::from_slice(self.bytes.borrow().get_ref()).unwrap()
    }
}

impl IBStream for StateStream {
    unsafe fn read(&self, buffer: *mut c_void, count: i32, read: *mut i32) -> tresult {
        if count < 0 || buffer.is_null() {
            return kResultFalse;
        }
        let output = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), count as usize) };
        let n = self.bytes.borrow_mut().read(output).unwrap();
        if !read.is_null() {
            unsafe {
                *read = n as i32;
            }
        }
        kResultOk
    }
    unsafe fn write(&self, buffer: *const c_void, count: i32, written: *mut i32) -> tresult {
        if count < 0 || buffer.is_null() {
            return kResultFalse;
        }
        let input = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), count as usize) };
        let n = self.bytes.borrow_mut().write(input).unwrap();
        if !written.is_null() {
            unsafe {
                *written = n as i32;
            }
        }
        kResultOk
    }
    unsafe fn seek(&self, offset: i64, mode: i32, result: *mut i64) -> tresult {
        let from = match mode {
            0 if offset >= 0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => return kResultFalse,
        };
        match self.bytes.borrow_mut().seek(from) {
            Ok(pos) => {
                if !result.is_null() {
                    unsafe {
                        *result = pos as i64;
                    }
                }
                kResultOk
            }
            Err(_) => kResultFalse,
        }
    }
    unsafe fn tell(&self, result: *mut i64) -> tresult {
        if result.is_null() {
            return kResultFalse;
        }
        unsafe {
            *result = self.bytes.borrow().position() as i64;
        }
        kResultOk
    }
}

#[test]
fn vst3_component_save_reload_preserves_embedded_configuration_before_and_after_activation() {
    let params = Parameters::default();
    params
        .apply_configuration(serde_json::to_value(crate::tests::changed_settings()).unwrap())
        .unwrap();
    let state = PluginState {
        version: AdaptiveJiPlugin::VERSION.into(),
        params: BTreeMap::new(),
        fields: params.serialize_fields(),
    };
    let input = StateStream::new(serde_json::to_vec(&state).unwrap());
    let source = Wrapper::<AdaptiveJiPlugin>::new();
    unsafe {
        assert_eq!(IComponent::set_state(&*source, input.shared()), kResultOk);
    }
    let saved = StateStream::new(Vec::new());
    unsafe {
        assert_eq!(IComponent::get_state(&*source, saved.shared()), kResultOk);
    }
    assert_eq!(saved.state().fields["engine"], state.fields["engine"]);

    for activate_first in [false, true] {
        let restored = Wrapper::<AdaptiveJiPlugin>::new();
        let setup = ProcessSetup {
            process_mode: 2,
            symbolic_sample_size: 0,
            max_samples_per_block: 512,
            sample_rate: 48000.0,
        };
        unsafe {
            assert_eq!(restored.setup_processing(&setup), kResultOk);
            if activate_first {
                assert_eq!(restored.set_active(1), kResultOk);
            }
            saved.bytes.borrow_mut().set_position(0);
            assert_eq!(IComponent::set_state(&*restored, saved.shared()), kResultOk);
            if !activate_first {
                assert_eq!(restored.set_active(1), kResultOk);
            }
        }
        let output = StateStream::new(Vec::new());
        unsafe {
            assert_eq!(
                IComponent::get_state(&*restored, output.shared()),
                kResultOk
            );
        }
        assert_eq!(output.state().fields["engine"], state.fields["engine"]);
        unsafe {
            assert_eq!(restored.set_active(0), kResultOk);
        }
    }
}
