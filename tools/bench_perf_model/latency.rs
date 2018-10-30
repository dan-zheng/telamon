//! Tests the latency model.
use telamon::device::{ArgMap, Context};
use telamon::helper::*;
use telamon::ir;
use telamon::search_space::{Action, DimKind, InstFlag, Order};
use PerfModelTest;

/// Tests the latency of an empty loop.
pub struct EmptyLoop;

impl EmptyLoop {
    const N: u32 = 1_000_000;
}

impl PerfModelTest for EmptyLoop {
    fn name() -> &'static str {
        "latency_empty_loop"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size = builder.param_size("n", Self::N);
        builder.open_dim_ex(size, DimKind::LOOP);
        builder.mov(&0i32);
        EmptyLoop
    }
}

/// Tests the latency of two nested empty loops.
pub struct TwoEmptyLoop {
    d0: ir::DimId,
    d1: ir::DimId,
}

impl TwoEmptyLoop {
    const N: u32 = 1000;
}

impl PerfModelTest for TwoEmptyLoop {
    fn name() -> &'static str {
        "latency_two_empty_loop"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size = builder.param_size("n", Self::N);
        let d0 = builder.open_dim_ex(size.clone(), DimKind::LOOP);
        let d1 = builder.open_dim_ex(size, DimKind::LOOP);
        builder.mov(&0i32);
        TwoEmptyLoop {
            d0: d0[0],
            d1: d1[0],
        }
    }

    fn get_actions(&self) -> Vec<Action> {
        let d0 = self.d0.into();
        let d1 = self.d1.into();
        vec![Action::Order(d0, d1, Order::OUTER)]
    }
}

/// Tests the latency of a small chain of instruction in a loop iteration.
pub struct InstChain;

impl InstChain {
    const N: u32 = 1_000_000;
}

impl PerfModelTest for InstChain {
    fn name() -> &'static str {
        "inst_chain"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("x", 1i32);
        builder.array::<i64>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size = builder.param_size("n", Self::N);
        let d0 = builder.open_dim_ex(size, DimKind::LOOP);
        let i0 = builder.mul(&"x", &"x");
        let i1 = builder.mul(&"x", &i0);
        let i2 = builder.mul(&"x", &i1);
        builder.close_dim(&d0);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(&"out", &Last(i2, &[&d0]), true, pattern, InstFlag::NO_CACHE);
        InstChain
    }
}

/// Tests the latency of a long chain of instruction in a loop iteration.
pub struct LongInstChain;

impl LongInstChain {
    const N: u32 = 10_000;
}

impl PerfModelTest for LongInstChain {
    fn name() -> &'static str {
        "long_inst_chain"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("x", 1i64);
        builder.array::<i64>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size = builder.param_size("n", Self::N);
        let d0 = builder.open_dim_ex(size, DimKind::LOOP);
        let mut inst = builder.mul(&"x", &"x");
        for _ in 1..100 {
            inst = builder.mul(&"x", &inst);
        }
        builder.close_dim(&d0);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(
            &"out",
            &Last(inst, &[&d0]),
            true,
            pattern,
            InstFlag::NO_CACHE,
        );
        LongInstChain
    }
}

/// Tests the latency on an unrolled reduction loop.
pub struct UnrollReduction {
    d0: ir::DimId,
    d1: ir::DimId,
}

impl UnrollReduction {
    const N: u32 = 10_000;
}

impl PerfModelTest for UnrollReduction {
    fn name() -> &'static str {
        "unroll_reduction"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("x", 1i32);
        builder.array::<i32>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let init = builder.mov(&"x");
        let d0_size = builder.param_size("n", Self::N);
        let d1_size = builder.cst_size(100);
        let d0 = builder.open_dim_ex(d0_size, DimKind::LOOP);
        let d1 = builder.open_dim_ex(d1_size, DimKind::UNROLL);
        let fby = builder.create_fby_variable(init, &[&d0, &d1]);
        let inst = builder.add(&"x", &fby);
        builder.set_loop_carried_variable(fby, inst);
        builder.close_dim(&d0);
        builder.close_dim(&d1);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(
            &"out",
            &Last(inst, &[&d0, &d1]),
            true,
            pattern,
            InstFlag::NO_CACHE,
        );
        UnrollReduction {
            d0: d0[0],
            d1: d1[0],
        }
    }

    fn get_actions(&self) -> Vec<Action> {
        let d0 = self.d0.into();
        let d1 = self.d1.into();
        vec![Action::Order(d0, d1, Order::OUTER)]
    }
}

/// Tests the latency when two loops are ordered sequentially.
pub struct OrderedLoops;

impl OrderedLoops {
    const SIZE: u32 = 1000;
}

impl PerfModelTest for OrderedLoops {
    fn name() -> &'static str {
        "ordered_loops"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::SIZE as i32);
        builder.scalar("k", Self::SIZE as i32);
        builder.scalar("m", Self::SIZE as i32);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size_n = builder.param_size("n", Self::SIZE);
        let size_k = builder.param_size("k", Self::SIZE);
        let size_m = builder.param_size("m", Self::SIZE);
        builder.open_dim_ex(size_n, DimKind::LOOP);
        let d1 = builder.open_dim_ex(size_k, DimKind::LOOP);
        builder.mov(&"n");
        builder.close_dim(&d1);
        let d2 = builder.open_dim_ex(size_m, DimKind::LOOP);
        builder.mov(&"n");
        builder.order(&d1, &d2, Order::BEFORE);
        OrderedLoops
    }
}

/// Tests the latency when two thread loops are ordered sequentially.
pub struct OrderedThreadDims;

impl OrderedThreadDims {
    const N: u32 = 1_000;

    const K: u32 = 100;
}

impl PerfModelTest for OrderedThreadDims {
    fn name() -> &'static str {
        "ordered_thread_dims"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("k", Self::K as i32);
        builder.scalar("x", 1i32);
        builder.array::<i32>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size_n = builder.param_size("n", Self::N);
        let size_0 = builder.cst_size(1024);
        let size_1 = builder.param_size("k", Self::K);
        let size_2 = builder.cst_size(64);
        builder.open_dim_ex(size_n, DimKind::LOOP);
        let d1 = builder.open_dim_ex(size_0.clone(), DimKind::THREAD);
        let pattern = ir::AccessPattern::Unknown(None);
        let init =
            builder.ld_ex(ir::Type::I(32), &"out", pattern, InstFlag::CACHE_GLOBAL);
        let d1_1 = builder.open_dim_ex(size_1.clone(), DimKind::LOOP);
        let d1_2 = builder.open_dim_ex(size_2.clone(), DimKind::UNROLL);
        let fby = builder.create_fby_variable(init, &[&d1_1, &d1_2]);
        let inst = builder.mul(&"x", &fby);
        builder.set_loop_carried_variable(fby, inst);
        builder.close_dim(&d1);
        builder.close_dim(&d1_1);
        builder.close_dim(&d1_2);

        let d2 = builder.open_dim_ex(size_0.clone(), DimKind::THREAD);
        let d2_1 = builder.open_dim_ex(size_1.clone(), DimKind::LOOP);
        let d2_2 = builder.open_dim_ex(size_2.clone(), DimKind::UNROLL);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(
            &"out",
            &Last(inst, &[&d1, &d1_1, &d1_2]),
            true,
            pattern,
            InstFlag::CACHE_GLOBAL,
        );

        builder.order(&d1, &d1_1, Order::OUTER);
        builder.order(&d1_1, &d1_2, Order::OUTER);
        builder.order(&d2, &d2_1, Order::OUTER);
        builder.order(&d2_1, &d2_2, Order::OUTER);
        builder.order(&d1, &d2, Order::BEFORE);
        OrderedThreadDims
    }
}

/// Test the latency in presence of point to point communication between loops.
pub struct DimMap;

impl DimMap {
    const N: u32 = 10_000;
}

impl PerfModelTest for DimMap {
    fn name() -> &'static str {
        "dim_map"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("x", 1i64);
        builder.array::<i64>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let size_0 = builder.param_size("n", Self::N);
        let size_1 = builder.cst_size(4);
        let init = builder.mov(&"x");
        let init2 = builder.mov(&"x");
        let d0 = builder.open_dim_ex(size_0, DimKind::LOOP);
        let d1 = builder.open_dim_ex(size_1.clone(), DimKind::UNROLL);
        let i0_fby = builder.create_fby_variable(init, &[&d0, &d1]);
        let i0 = builder.mul(&i0_fby, &"x");
        builder.set_loop_carried_variable(i0_fby, i0);
        let i1 = builder.mov(&i0);
        builder.close_dim(&d1);
        let d2 = builder.open_dim_ex(size_1, DimKind::UNROLL);
        let op = builder.dim_map(i1, &[(&d1, &d2)], ir::DimMapScope::Thread);
        let i2_fby = builder.create_fby_variable(init2, &[&d0, &d2]);
        let i2 = builder.mad(&op, &op, &i2_fby);
        builder.set_loop_carried_variable(i2_fby, i2);
        builder.close_dim(&d2);
        builder.close_dim(&d0);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(
            &"out",
            &Last(i2, &[&d0, &d2]),
            true,
            pattern,
            InstFlag::NO_CACHE,
        );
        builder.order(&d1, &d2, Order::BEFORE);
        DimMap
    }
}

/// Test a latency that depends on the operand position, in the slow position.
pub struct OperandPositionSlow;

impl OperandPositionSlow {
    const N: u32 = 10_000;
}

impl PerfModelTest for OperandPositionSlow {
    fn name() -> &'static str {
        "operand_position_slow"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("x", 1i64);
        builder.array::<i64>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let init = builder.mov(&"x");
        let d0_size = builder.param_size("n", Self::N);
        let d1_size = builder.cst_size(100);
        let d0 = builder.open_dim_ex(d0_size, DimKind::LOOP);
        let d1 = builder.open_dim_ex(d1_size, DimKind::UNROLL);
        let fby = builder.create_fby_variable(init, &[&d0, &d1]);
        let inst = builder.mad(&fby, &"x", &"x");
        builder.set_loop_carried_variable(fby, inst);
        builder.close_dim(&d0);
        builder.close_dim(&d1);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(
            &"out",
            &Last(inst, &[&d0, &d1]),
            true,
            pattern,
            InstFlag::NO_CACHE,
        );
        builder.order(&d0, &d1, Order::OUTER);
        OperandPositionSlow
    }
}

/// Test a latency that depends on the operand position, in the fast position.
pub struct OperandPositionFast;

impl OperandPositionFast {
    const N: u32 = 10_000;
}

impl PerfModelTest for OperandPositionFast {
    fn name() -> &'static str {
        "operand_position_fast"
    }

    fn gen_signature<AM: ArgMap + Context>(builder: &mut SignatureBuilder<AM>) {
        builder.scalar("n", Self::N as i32);
        builder.scalar("x", 1i64);
        builder.array::<i64>("out", 1);
    }

    fn gen_function(builder: &mut Builder) -> Self {
        let init = builder.mov(&"x");
        let d0_size = builder.param_size("n", Self::N);
        let d1_size = builder.cst_size(100);
        let d0 = builder.open_dim_ex(d0_size, DimKind::LOOP);
        let d1 = builder.open_dim_ex(d1_size, DimKind::UNROLL);
        let fby = builder.create_fby_variable(init, &[&d0, &d1]);
        let inst = builder.mad(&"x", &"x", &fby);
        builder.set_loop_carried_variable(fby, inst);
        builder.close_dim(&d0);
        builder.close_dim(&d1);
        let pattern = ir::AccessPattern::Unknown(None);
        builder.st_ex(
            &"out",
            &Last(inst, &[&d0, &d1]),
            true,
            pattern,
            InstFlag::NO_CACHE,
        );
        builder.order(&d0, &d1, Order::OUTER);
        OperandPositionFast
    }
}

// TODO(test): syncthread.
// TODO(test): mixed inst chain. (mixing add/mul/..)
// TODO(test): loads and stores
// TODO(test): temporary loads and stores.
