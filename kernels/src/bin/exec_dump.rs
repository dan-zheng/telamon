use std::env;
use std::io::Read;
use telamon::device::{ArgMap, Context};
use telamon_kernels::{linalg, Kernel};
use telamon_x86 as x86;

fn dispatch_exec<'a, C: Context + ArgMap<'a>, R: Read>(
    file: &mut R,
    kernel_name: &str,
    context: &mut C,
) {
    match kernel_name as &str {
        "axpy" => linalg::Axpy::<f32>::execute_dump(context, file),
        "mv" => linalg::MatVec::<f32>::execute_dump(context, file),
        "gesummv" => linalg::Gesummv::<f32>::execute_dump(context, file),
        "matmul" => linalg::MatMul::<f32>::execute_dump(context, file),
        _ => panic!("Valid kernel names are: axpy, mv, gesummv, matmul"),
    }
}

#[cfg(not(feature = "mppa"))]
fn call_mppa(_: &str, _: &str) {}

#[cfg(feature = "mppa")]
fn call_mppa(kernel_name: &str, dump_path: &str) {
    use telamon_mppa as mppa;
    let mut context = mppa::Context::default();
    let mut file = std::fs::File::open(dump_path).expect("Invalid dump path");
    dispatch_exec(&mut file, kernel_name, &mut context);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    assert!(args.len() == 3);
    let kernel_name = &args[1];
    let binary_path = &args[2];

    let _ = env_logger::try_init();
    let mut file = std::fs::File::open(binary_path).expect("Invalid dump path");
    if cfg!(feature = "mppa") {
        call_mppa(kernel_name, binary_path);
    } else {
        let mut context = x86::Context::default();
        dispatch_exec(&mut file, kernel_name, &mut context);
    }
}
