//! MPPA evaluation context.
use telamon::codegen::{self, ParamVal};
use telamon::device::Context as ContextTrait;
use telamon::device::{self, ArrayArgument, AsyncCallback, EvalMode, ScalarArgument};
use telamon::explorer;
use telamon::ir;
use crate::printer::MppaPrinter;
use crate::{mppa, Namer};
use crossbeam;
use crossbeam::queue::ArrayQueue;
use itertools::Itertools;
use lazy_static::lazy_static;
use libc;
use std;
use std::sync::{mpsc, Arc, atomic::{AtomicUsize, Ordering}};
use std::time::Instant;
use telajax;
use utils::unwrap;
use utils::*;

lazy_static!{static ref ATOMIC_USIZE: AtomicUsize = AtomicUsize::new(0);}
const EXECUTION_QUEUE_SIZE: usize = 32;

pub trait Argument: Sync + Send {
    /// Returns a pointer to the object.
    fn raw_ptr(&self) -> *const libc::c_void;
    /// Returns the argument value if it can represent a size.
    fn as_size(&self) -> Option<u32> { None }
}

impl<'a> Argument for Box<dyn ScalarArgument + 'a>
{
    fn raw_ptr(&self) -> *const libc::c_void { device::ScalarArgument::raw_ptr(&**self as &dyn ScalarArgument) }

    fn as_size(&self) -> Option<u32> { device::ScalarArgument::as_size(&**self as &dyn ScalarArgument) }
}

struct MppaArray(telajax::Buffer<i8>);

impl MppaArray {
    pub fn new(executor: &'static telajax::Device, len: usize) -> Self {
        MppaArray (
            telajax::Buffer::new(executor, len)
        )
    }
}

impl device::ArrayArgument for MppaArray {
    fn read_i8(&self) -> Vec<i8> {
        self.0.read().unwrap()
    }

    fn write_i8(&self, slice: &[i8]) {
        self.0.write(slice).unwrap();
    }
}

impl Argument for MppaArray {
    fn as_size(&self) -> Option<u32> { Some(self.0.len as u32) }

    fn raw_ptr(&self) -> *const libc::c_void { self.0.raw_ptr() }
}

/// MPPA evaluation context.
pub struct Context {
    device: mppa::Mppa,
    executor: &'static telajax::Device,
    parameters: FnvHashMap<String, Arc<Argument >>,
    //wrappers: Cache<ir::Signature, telajax::Wrapper>,
    writeback_slots: ArrayQueue<MppaArray>,
}
unsafe impl Sync for Context {}

/// We need to keep the arguments allocated for the kernel somewhere
enum KernelArg {
    GlobalMem(MppaArray),
    Size(u32),
    External(*const libc::c_void),
}

impl KernelArg {
    fn raw_ptr(&self) -> *const libc::c_void {
        match self {
            KernelArg::GlobalMem(mem) => mem.raw_ptr(),
            KernelArg::Size(size) => size as *const _ as *const libc::c_void,
            KernelArg::External(ptr) => *ptr,
        }
    }
}

impl Context {
    /// Creates a new `Context`. Blocks until the MPPA device is ready to be
    /// used.
    pub fn new() -> Self {
        let executor = telajax::Device::get();
        let writeback_slots = ArrayQueue::new(EXECUTION_QUEUE_SIZE);
        for _ in 0..EXECUTION_QUEUE_SIZE {
            writeback_slots.push(MppaArray::new(executor, 8)).unwrap();
        }
        Context {
            device: mppa::Mppa::default(),
            executor,
            parameters: FnvHashMap::default(),
            //wrappers: Cache::new(25),
            writeback_slots,
        }
    }

    fn bind_param(&mut self, name: String, value: Arc<Argument>) {
        self.parameters.insert(name, value);
    }

    /// Compiles and sets the arguments of a kernel.
    fn setup_kernel(
        &self,
        fun: &codegen::Function,
        id: usize,
    ) -> (telajax::Kernel, Vec<KernelArg>)
    {
        let mut printer = MppaPrinter::default();
        let kernel_code = printer.wrapper_function(fun, id);
        std::fs::write("dump_kernel.c", &kernel_code).expect("Could not read file");
        //println!("KERNEL CODE\n{}", kernel_code);
        let wrapper = self.get_wrapper(fun, id);
        let cflags = std::ffi::CString::new("-mhypervisor").unwrap();
        let lflags = std::ffi::CString::new("-mhypervisor -lutask -lvbsp").unwrap();
        let kernel_code = unwrap!(std::ffi::CString::new(kernel_code));
        let mut kernel =
            self.executor
                .build_kernel(&kernel_code, &cflags, &lflags, &*wrapper)
                .unwrap();
        kernel.set_num_clusters(1).unwrap();
        let mut namer = Namer::default();
        let name_map = codegen::NameMap::new(fun, &mut namer);
        let (mut arg_sizes, mut kernel_args): (Vec<_>, Vec<_>) = fun
            .device_code_args()
            .map(|p| {
                match p {
                    ParamVal::External(..) => {
                        let name = name_map.name_param(p.key());
                        let arg = self.get_param(&name);
                        (get_type_size(p.t()), KernelArg::External(arg.raw_ptr()))
                    }
                    ParamVal::GlobalMem(_, size, _) => {
                        let size = self.eval_size(size);
                        let mem = MppaArray::new(self.executor, size as usize);
                        (
                            telajax::Mem::get_mem_size(),
                            KernelArg::GlobalMem(mem),
                        )
                    }
                    ParamVal::Size(size) => {
                        let size = self.eval_size(size);
                        //println!("param size {} : {}", name, size);
                        (get_type_size(p.t()), KernelArg::Size(size))
                    }
                }
            })
            .unzip();
        let out_mem = self.writeback_slots.pop().unwrap();
        kernel_args.push(KernelArg::GlobalMem(out_mem));
        arg_sizes.push(telajax::Mem::get_mem_size());
        let args_ptr = kernel_args
            .iter()
            .map(|k_arg| k_arg.raw_ptr())
            .collect_vec();
        kernel.set_args(&arg_sizes[..], &args_ptr[..]).unwrap();
        (kernel, kernel_args)
    }

    /// Returns the wrapper for the given signature.
    fn get_wrapper(&self, fun: &codegen::Function, id: usize) -> Arc<telajax::Wrapper> {
        // TODO: There was a memoization here that allowed to cache a result for an
        // already generated signature wrapper. Maybe reimplement it
        let mut printer = MppaPrinter::default();
        let mut namer = Namer::default();
        let mut name_map = codegen::NameMap::new(fun, &mut namer);
        let ocl_code = printer.print_ocl_wrapper(fun, &mut name_map, id);
        //println!("{}", ocl_code);
        let name = std::ffi::CString::new(format!("wrapper_{}", id)).unwrap();
        let ocl_code = std::ffi::CString::new(ocl_code).unwrap();
        Arc::new(self.executor.build_wrapper(&name, &ocl_code).unwrap())
    }

    /// Returns a parameter given its name.
    pub fn get_param(&self, name: &str) -> &Argument { self.parameters[name].as_ref() }
}

fn get_type_size(t: ir::Type) -> usize {
    match t {
        ir::Type::I(u) | ir::Type::F(u) => (u / 8) as usize,
        ir::Type::PtrTo(_) => telajax::Mem::get_mem_size(),
    }
}

impl device::Context for Context {
    fn device(&self) -> &device::Device { &self.device }

    fn benchmark(&self, _function: &codegen::Function, _num_samples: usize) -> Vec<f64> {
        unimplemented!()
    }

    fn evaluate(&self, fun: &codegen::Function, _mode: EvalMode) -> Result<f64, ()> {
        let id = ATOMIC_USIZE.fetch_add(1, Ordering::SeqCst);
        let (mut kernel, mut kernel_args) = self.setup_kernel(fun, id);
        // TODO Use a proper id
        self.executor.execute_kernel_id(&mut kernel, id).unwrap();
        let out_mem = if let KernelArg::GlobalMem(mem) = kernel_args.pop().unwrap() {
            mem
        } else {
            panic!()
        };
        let ptr_i8 = out_mem.read_i8().as_ptr();
        let res: f64 = unsafe { *std::mem::transmute::<*const i8, *const f64>(ptr_i8) };
        self.writeback_slots.push(out_mem).unwrap();
        Ok(res)
    }

    fn async_eval<'c, 'd>(
        &self,
        num_workers: usize,
        _mode: EvalMode,
        inner: &(Fn(&mut device::AsyncEvaluator<'c, 'd>) + Sync),
    )
    {
        // FIXME: execute in parallel
        let (send, recv) = mpsc::sync_channel(EXECUTION_QUEUE_SIZE);
        crossbeam::scope(move |scope| {
            // Start the explorer threads.
            for _ in 0..num_workers {
                let mut evaluator = AsyncEvaluator {
                    context: self,
                    sender: send.clone(),
                };
                unwrap!(
                    scope
                        .builder()
                        .name("Telamon - Explorer Thread".to_string())
                        .spawn(move |_| inner(&mut evaluator))
                );
            }
            // Start the evaluation thread.
            let eval_thread_name = "Telamon - CPU Evaluation Thread".to_string();
            unwrap!(scope.builder().name(eval_thread_name).spawn(move |_| {
                while let Ok((candidate, mut kernel, callback, id)) = recv.recv() {
                    // TODO: measure time directly on MPPA
                    let t0 = Instant::now();
                    self.executor.execute_kernel_id(&mut kernel, id).unwrap();
                    let t = Instant::now() - t0;
                    callback.call(candidate, t.subsec_nanos() as f64);
                }
            }));
        }).unwrap();
    }

    fn param_as_size(&self, name: &str) -> Option<u32> { self.get_param(name).as_size() }
}

impl<'a> device::ArgMap<'a> for Context {
    fn bind_erased_scalar(&mut self, param: &ir::Parameter, value: Box<dyn ScalarArgument>) {
        assert_eq!(param.t, value.get_type());
        self.bind_param(param.name.clone(), Arc::new(value));
    }

    fn bind_erased_array(
        &mut self,
        param: &ir::Parameter,
        t: ir::Type,
        len: usize,
    ) -> Arc<dyn ArrayArgument + 'a>
    {
        let size = len * unwrap!(t.len_byte()) as usize;
        let buffer_arc = Arc::new(MppaArray::new(self.executor, size));
        self.bind_param(param.name.clone(), Arc::clone(&buffer_arc) as Arc<Argument>);
        buffer_arc
    }
}

type AsyncPayload<'a, 'b> = (
    explorer::Candidate<'a>,
    telajax::Kernel,
    AsyncCallback<'a, 'b>,
    usize,
);

/// Asynchronous evaluator.
struct AsyncEvaluator<'a, 'b>
where 'a: 'b
{
    context: &'b Context,
    sender: mpsc::SyncSender<AsyncPayload<'a, 'b>>,
}

impl<'a, 'b, 'c> device::AsyncEvaluator<'a, 'c> for AsyncEvaluator<'a, 'b>
where
    'a: 'b,
    'c: 'b,
{
    fn add_kernel(
        &mut self,
        candidate: explorer::Candidate<'a>,
        callback: device::AsyncCallback<'a, 'b>,
    )
    {
        let id = ATOMIC_USIZE.fetch_add(1, Ordering::SeqCst);
        let (kernel, _) = {
            let dev_fun = codegen::Function::build(&candidate.space);
            self.context.setup_kernel(&dev_fun, id)
        };
        unwrap!(self.sender.send((candidate, kernel, callback, id)));
    }
}
