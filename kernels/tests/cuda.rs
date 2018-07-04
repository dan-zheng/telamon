#![cfg(feature="cuda")]
extern crate env_logger;
extern crate telamon;
extern crate telamon_kernels;

use telamon::device::cuda;
use telamon_kernels::{Kernel, linalg};
use telamon::explorer::config::Config;

macro_rules! test_output {
    ($name:ident, $kernel:ty, $num_tests:expr, $params:expr) => {
        #[test]
        fn $name() {
            let _ = env_logger::try_init();
            let executor = cuda::Executor::init();
            let mut context = cuda::Context::new(&executor);
            let config = Config::read_from_file();
            <$kernel>::find_best(&config, $params, &mut context);
        }
    }
}

test_output!(axpy, linalg::Axpy<f32>, 100, (1 << 15, true));
test_output!(mv, linalg::MatVec<f32>, 100, (1<<4, 1<<2, true));
test_output!(gesummv, linalg::Gesummv<f32>, 100, (1<<4, 1<<4, true));
test_output!(matmul, linalg::MatMul<f32>, 100, linalg::MatMulP::new(16, 16, 16));
