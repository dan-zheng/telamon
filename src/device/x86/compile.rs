use libc;
use libloading;
use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::process::{Command, ExitStatus};
use std::time::Instant;
use utils::unwrap;

pub fn compile(mut source_file: File, lib_path: &str) -> ExitStatus {
    unwrap!(source_file.seek(SeekFrom::Start(0)));
    Command::new("gcc")
        .stdin(source_file)
        .arg("-shared")
        .arg("-fPIC")
        .arg("-o")
        .arg(lib_path)
        .arg("-xc")
        .arg("-")
        .arg("-lpthread")
        .status()
        .expect("Could not execute gcc")
}

pub fn link_and_exec(
    lib_path: &str,
    fun_name: &str,
    mut args: Vec<*mut libc::c_void>,
) -> f64 {
    let lib = libloading::Library::new(lib_path).expect("Library not found");
    unsafe {
        let func: libloading::Symbol<unsafe extern "C" fn(*mut *mut libc::c_void)> = lib
            .get(fun_name.as_bytes())
            .expect("Could not find symbol in library");
        let t0 = Instant::now();
        func(args.as_mut_ptr());
        let t = Instant::now() - t0;
        f64::from(t.subsec_nanos())
    }
}
