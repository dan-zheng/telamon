//! Unsafe wrapper around  library from Kalray.
#![allow(dead_code)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
use device::{self, ArrayArgument};
use itertools::{zip_eq, Itertools};
use libc;
use parking_lot;
use std;
use std::ffi::CStr;
use std::sync::{Arc, RwLock};

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

lazy_static! {
    static ref DEVICE: Device = Device::init();
}

lazy_static! {
    static ref MEM_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

unsafe impl Sync for Device {}
unsafe impl Send for Device {}

/// Buffer in MPPA RAM.
pub struct Buffer {
    pub mem: RwLock<Mem>,
    pub size: usize,
    pub executor: &'static Device,
}

impl Buffer {
    pub fn new(executor: &'static Device, len: usize) -> Self {
        let mem_block = executor.alloc(len);
        Buffer {
            mem: RwLock::new(mem_block),
            size: len,
            executor,
        }
    }

    pub fn raw_ptr(&self) -> *const libc::c_void { self.mem.read().unwrap().raw_ptr() }
}
impl device::ArrayArgument for Buffer {
    fn read_i8(&self) -> Vec<i8> {
        let mem_block = unwrap!(self.mem.read());
        let mut read_buffer = vec![0; mem_block.len()];
        self.executor
            .read_buffer::<i8>(&mem_block, &mut read_buffer, &[]);
        read_buffer
    }

    fn write_i8(&self, slice: &[i8]) {
        let mut mem_block = unwrap!(self.mem.write());
        self.executor.write_buffer::<i8>(slice, &mut mem_block, &[]);
    }
}
/// A Telajax execution context.
pub struct Device {
    inner: device_t,
    rwlock: Box<parking_lot::RwLock<()>>,
}

impl Device {
    /// Returns a reference to the `Device`. As Telajax implementation is
    /// supposed to be thread-safe, it should be therefore safe to call
    /// this api from different threads. These calls are done through a
    /// static immutable reference
    pub fn get() -> &'static Device { &DEVICE }

    /// Initializes the device.
    fn init() -> Self {
        let mut error = 0;
        let device = unsafe {
            Device {
                inner: telajax_device_init(0, [].as_mut_ptr(), &mut error),
                rwlock: Box::new(parking_lot::RwLock::default()),
            }
        };
        assert_eq!(error, 0);
        device
    }

    pub fn allocate_array(&'static self, len: usize) -> Buffer { Buffer::new(self, len) }

    /// Build a wrapper for a kernel.
    pub fn build_wrapper(&self, name: &CStr, code: &CStr) -> Wrapper {
        let mut error = 0;
        let flags: &'static CStr = Default::default();
        let wrapper = unsafe {
            telajax_wrapper_build(
                name.as_ptr(),
                code.as_ptr(),
                flags.as_ptr(),
                &self.inner as *const _ as *mut _,
                &mut error,
            )
        };
        assert_eq!(error, 0);
        Wrapper(wrapper)
    }

    /// Compiles a kernel.
    pub fn build_kernel(
        &self,
        code: &CStr,
        cflags: &CStr,
        lflags: &CStr,
        wrapper: &Wrapper,
    ) -> Kernel
    {
        // FIXME: disable -O3
        // TODO(cc_perf): precompile headers
        // TODO(cc_perf): use double pipes in telajax
        // TODO(cc_perf): avoid collisions in kernel evaluations
        let mut error = 0;
        let _lock = MEM_MUTEX.lock().unwrap();
        let kernel = unsafe {
            telajax_kernel_build(
                code.as_ptr(),
                cflags.as_ptr(),
                lflags.as_ptr(),
                &wrapper.0 as *const _ as *mut _,
                &self.inner as *const _ as *mut _,
                &mut error,
            )
        };
        if error != 0 {
            std::mem::drop(_lock);
        }
        assert_eq!(error, 0);
        Kernel(kernel)
    }

    /// Asynchronously executes a `Kernel`.
    pub fn enqueue_kernel(&self, kernel: &mut Kernel) -> Event {
        let _lock = MEM_MUTEX.lock().unwrap();
        unsafe {
            let mut event_ptr = std::mem::uninitialized();
            let err = telajax_kernel_enqueue(
                &mut kernel.0 as *mut _,
                &self.inner as *const _ as *mut _,
                &mut event_ptr,
            );
            if err != 0 {
                std::mem::drop(_lock);
                assert_eq!(err, 0);
            }
            Event(event_ptr)
        }
    }

    /// Print func id then execute it
    pub fn execute_kernel_id(&self, kernel: &mut Kernel, kernel_id: u16) {
        println!("Executing kernel {}", kernel_id);
        self.execute_kernel(kernel);
    }

    /// Executes a `Kernel` and then wait for completion.
    pub fn execute_kernel(&self, kernel: &mut Kernel) {
        unsafe {
            // We MUST make sure that drop is called on lock before it is called on
            // event, else we will get a deadlock as event drop will try to
            // take the lock
            let mut event: Event = std::mem::uninitialized();
            let _lock = MEM_MUTEX.lock().unwrap();
            let err = telajax_kernel_enqueue(
                &mut kernel.0 as *mut _,
                &self.inner as *const _ as *mut _,
                &mut event.0,
            );
            if err != 0 {
                // Apparently destructor calls in panic is not reliable (to be confirmed,
                // but rust explictly tells that there is no guarantee
                // that a destructor shall be called in every cases -
                // maybe limited to abort though)... We must at least explicitly call
                // drop on _lock before dropping event as event drop will try to take the
                // lock in turn.
                std::mem::drop(_lock);
                panic!("error in execute_kernel after kernel_enqueue");
            }
            let err = telajax_event_wait(1, &event.0 as *const _);
            if err != 0 {
                std::mem::drop(_lock);
                panic!("error in event_wait");
            }
        }
    }

    /// Waits until all kernels have completed their execution.
    pub fn wait_all(&self) {
        let _ = self.rwlock.write();
        unsafe {
            assert_eq!(telajax_device_waitall(&self.inner as *const _ as *mut _), 0);
        }
    }

    /// Allocates a memory buffer.
    pub fn alloc(&self, size: usize) -> Mem {
        let mut error = 0;
        let _lock = MEM_MUTEX.lock().unwrap();
        let mem = unsafe {
            telajax_device_mem_alloc(
                size,
                1 << 0,
                &self.inner as *const _ as *mut _,
                &mut error,
            )
        };
        assert_eq!(error, 0);
        Mem {
            ptr: mem,
            len: size,
        }
    }

    /// Asynchronously copies a buffer to the device.
    pub fn async_write_buffer<T: Copy>(
        &self,
        data: &[T],
        mem: &mut Mem,
        wait_events: &[Event],
    ) -> Event
    {
        let size = data.len() * std::mem::size_of::<T>();
        assert!(size <= mem.len);
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let wait_n = wait_events.len() as libc::c_uint;
        let wait_ptr = wait_events.as_ptr() as *const event_t;
        unsafe {
            let event_ptr: *mut _cl_event = std::mem::uninitialized();
            let mut event = Event::new(event_ptr);
            let _lock = MEM_MUTEX.lock().unwrap();
            let res = telajax_device_mem_write(
                &self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                wait_n,
                wait_ptr,
                &mut event.0,
            );
            if res != 0 {
                // Apparently destructor calls in panic is not reliable (to be confirmed,
                // but rust explicitely tells that there is no guarantee
                // that a destructor shall be called in every cases -
                // maybe limited to abort though)...  See this : https://github.com/rust-lang/rfcs/pull/1066
                // We must at least explicitly call drop on _lock before dropping event
                // as event drop will try to take the lock in turn, thus
                // causing a deadlock.
                std::mem::drop(_lock);
                panic!("error in mem write");
            }
            assert_eq!(res, 0);
            event
        }
    }

    /// Copies a buffer to the device.
    pub fn write_buffer<T: Copy>(
        &self,
        data: &[T],
        mem: &mut Mem,
        wait_events: &[Event],
    )
    {
        let size = data.len() * std::mem::size_of::<T>();
        assert!(size <= mem.len);
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let wait_n = wait_events.len() as libc::c_uint;
        let wait_ptr = if wait_n == 0 {
            std::ptr::null() as *const event_t
        } else {
            wait_events.as_ptr() as *const event_t
        };
        let _lock = MEM_MUTEX.lock().unwrap();
        unsafe {
            let null_mut = std::ptr::null_mut();
            let res = telajax_device_mem_write(
                &self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                wait_n,
                wait_ptr,
                null_mut,
            );
            assert_eq!(res, 0);
        }
    }

    /// Asynchronously copies a buffer from the device.
    pub fn async_read_buffer<T: Copy>(
        &self,
        mem: &Mem,
        data: &mut [T],
        wait_events: &[Event],
    ) -> Event
    {
        let size = data.len() * std::mem::size_of::<T>();
        assert!(size <= mem.len);
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let wait_n = wait_events.len() as std::os::raw::c_uint;
        let _lock = MEM_MUTEX.lock().unwrap();
        let wait_ptr = if wait_n == 0 {
            std::ptr::null() as *const event_t
        } else {
            wait_events.as_ptr() as *const event_t
        };
        unsafe {
            let mut event: Event = std::mem::uninitialized();
            let res = telajax_device_mem_read(
                &self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                wait_n,
                wait_ptr,
                &mut event.0,
            );
            assert_eq!(res, 0);
            event
        }
    }

    /// Copies a buffer from the device.
    pub fn read_buffer<T: Copy>(&self, mem: &Mem, data: &mut [T], wait_events: &[Event]) {
        let size = data.len() * std::mem::size_of::<T>();
        assert!(size <= mem.len);
        let data_ptr = data.as_ptr() as *mut std::os::raw::c_void;
        let null_mut = std::ptr::null_mut();
        let wait_n = wait_events.len() as std::os::raw::c_uint;
        let _lock = MEM_MUTEX.lock().unwrap();
        let wait_ptr = if wait_n == 0 {
            std::ptr::null() as *const event_t
        } else {
            wait_events.as_ptr() as *const event_t
        };
        unsafe {
            let res = telajax_device_mem_read(
                &self.inner as *const _ as *mut _,
                mem.ptr,
                data_ptr,
                size,
                wait_n,
                wait_ptr,
                null_mut,
            );
            assert_eq!(res, 0);
        }
    }

    /// Set a callback to call when an event is triggered.
    pub fn set_event_callback<F>(&self, event: &Event, closure: F)
    where F: FnOnce() + Send {
        let callback_data = CallbackData {
            closure,
            rwlock: &*self.rwlock,
        };
        let data_ptr = Box::into_raw(Box::new(callback_data));
        let callback = callback_wrapper::<F>;
        let _lock = MEM_MUTEX.lock().unwrap();
        unsafe {
            self.rwlock.raw_read();
            let data_ptr = data_ptr as *mut std::os::raw::c_void;
            telajax_event_set_callback(Some(callback), data_ptr, event.0);
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.rwlock.write();
        unsafe {
            telajax_device_finalize(&mut self.inner);
        }
        debug!("MPPA device finalized");
    }
}

pub struct Wrapper(wrapper_s);
unsafe impl Send for Wrapper {}
unsafe impl Sync for Wrapper {}

impl Drop for Wrapper {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(telajax_wrapper_release(&mut self.0 as *mut _), 0);
        }
    }
}

pub struct Kernel(kernel_s);

impl Kernel {
    /// Sets the arguments of the `Kernel`.
    pub fn set_args(&mut self, sizes: &[usize], args: &[*const libc::c_void]) {
        assert_eq!(sizes.len(), args.len());
        let num_arg = sizes.len() as i32;
        let sizes_ptr = sizes.as_ptr();
        unsafe {
            // Needs *mut ptr mostly because they were not specified as const in original
            // c api
            assert_eq!(
                telajax_kernel_set_args(
                    num_arg,
                    sizes_ptr as *const _ as *mut _,
                    args.as_ptr() as *const _ as *mut _,
                    &mut self.0 as *mut _
                ),
                0
            );
        }
    }

    /// Sets the number of clusters that must execute the `Kernel`.
    pub fn set_num_clusters(&mut self, num: usize) {
        assert!(num <= 16);
        unsafe {
            assert_eq!(
                telajax_kernel_set_dim(
                    1,
                    [1].as_ptr(),
                    [num].as_ptr(),
                    &mut self.0 as *mut _
                ),
                0
            );
        }
    }
}

unsafe impl Send for Kernel {}

impl Drop for Kernel {
    fn drop(&mut self) {
        unsafe {
            assert_eq!(telajax_kernel_release(&mut self.0 as *mut _), 0);
        }
    }
}

/// A buffer allocated on the device.
pub struct Mem {
    ptr: mem_t,
    len: usize,
}

impl Mem {
    pub fn get_mem_size() -> usize { std::mem::size_of::<mem_t>() }

    pub fn raw_ptr(&self) -> *const libc::c_void {
        &self.ptr as *const _ as *const libc::c_void
    }

    pub fn len(&self) -> usize { self.len }
}

unsafe impl Sync for Mem {}
unsafe impl Send for Mem {}

impl Drop for Mem {
    fn drop(&mut self) {
        unsafe {
            let _lock = MEM_MUTEX.lock().unwrap();
            assert_eq!(telajax_device_mem_release(self.ptr), 0);
        }
    }
}

/// An event triggered at the end of a memory operation or kernel execution.
#[repr(C)]
pub struct Event(*mut _cl_event);

impl Event {
    fn new(ev_ptr: *mut _cl_event) -> Self { Event(ev_ptr) }
}

impl Drop for Event {
    fn drop(&mut self) {
        let _lock = MEM_MUTEX.lock().unwrap();
        unsafe {
            telajax_event_release(self.0);
        }
    }
}

/// Calls the closure passed in data.
unsafe extern "C" fn callback_wrapper<F: FnOnce()>(
    _: cl_event,
    _: i32,
    data: *mut std::os::raw::c_void,
)
{
    let data = Box::from_raw(data as *mut CallbackData<F>);
    let ref lock = *data.rwlock;
    (data.closure)();
    lock.raw_unlock_read();
}

type Callback = unsafe extern "C" fn(*const libc::c_void, i32, *mut libc::c_void);

struct CallbackData<F: FnOnce()> {
    closure: F,
    rwlock: *const parking_lot::RwLock<()>,
}
