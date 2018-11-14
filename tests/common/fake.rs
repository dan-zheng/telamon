#![allow(dead_code)]
//! Provides a fake implementations of device traits for testing.
use std::f64;
use std::io::Write;
use std::sync::Arc;
use telamon::codegen;
use telamon::device::{self, ArrayArgument, ScalarArgument};
use telamon::explorer::Candidate;
use telamon::ir::{self, Operator};
use telamon::model::{self, HwPressure};
use telamon::search_space::*;
use utils::*;

use std::marker::PhantomData;
/// A fake device.
pub struct Device {
    pub shared_mem_size: u32,
}

impl Default for Device {
    fn default() -> Device {
        Device {
            shared_mem_size: 1 << 17,
        }
    }
}

impl device::Device for Device {
    fn name(&self) -> &str {
        "fake_device"
    }

    fn print(&self, _: &codegen::Function, _: &mut Write) {}

    fn check_type(&self, _: ir::Type) -> Result<(), ir::TypeError> {
        Ok(())
    }

    fn max_unrolling(&self) -> u32 {
        256
    }

    /// Indicates which operators can be vectorized on a dimension. We only allow memory
    /// operators and `Add` to be vectorized (to be able to test both vectorizable and
    /// non-vectorizable operations).
    fn can_vectorize(&self, dim: &ir::Dimension, op: &ir::Operator) -> bool {
        match op {
            Operator::TmpLd(..)
            | Operator::TmpSt(..)
            | Operator::BinOp(ir::BinOp::Add, ..) => true,
            Operator::Ld(.., pattern)
            | Operator::St(.., pattern)
            | Operator::DmaStart {
                src_pattern: pattern,
                ..
            }
            | Operator::DmaWait {
                dst_pattern: pattern,
                ..
            } => pattern.is_layout_dimension(dim.id()),
            _ => false,
        }
    }

    fn max_vectorization(&self, op: &ir::Operator) -> [u32; 2] {
        match op {
            Operator::DmaStart { .. } | Operator::DmaWait { .. } => [std::u32::MAX; 2],
            _ => [8, 4],
        }
    }

    fn has_vector_registers(&self) -> bool {
        true
    }

    fn max_block_dims(&self) -> u32 {
        3
    }

    fn max_threads(&self) -> u32 {
        1024
    }

    fn max_inner_block_size(&self) -> u32 {
        65535
    }

    fn shared_mem(&self) -> u32 {
        self.shared_mem_size
    }

    fn pointer_type(&self, _: MemSpace) -> ir::Type {
        ir::Type::I(32)
    }

    // Warning: this assumes only global memory accesses can use caches.
    fn supported_mem_flags(&self, op: &ir::Operator) -> InstFlag {
        match op {
            // Only accesses to external memory blocks can be non-coherent.
            ir::Operator::Ld(.., pat) if pat.mem_block().is_none() => InstFlag::ALL,
            ir::Operator::Ld(..)
            | ir::Operator::St(..)
            | ir::Operator::TmpLd(..)
            | ir::Operator::TmpSt(..)
            | ir::Operator::DmaStart { .. }
            | ir::Operator::DmaWait { .. } => InstFlag::COHERENT,
            _ => panic!("invalid memory access operator"),
        }
    }

    fn lower_type(&self, t: ir::Type, _: &SearchSpace) -> Option<ir::Type> {
        Some(t)
    }

    fn loop_iter_pressure(&self, _: DimKind) -> (HwPressure, HwPressure) {
        (HwPressure::zero(self), HwPressure::zero(self))
    }

    fn hw_pressure(
        &self,
        _: &SearchSpace,
        _: &HashMap<ir::DimId, model::size::Range>,
        _: &HashMap<ir::StmtId, model::Nesting>,
        _: &ir::Statement,
        _: &device::Context,
    ) -> HwPressure {
        HwPressure::zero(self)
    }

    fn bottlenecks(&self) -> &[&'static str] {
        &["issue", "alu", "mem"]
    }

    fn block_parallelism(&self, _: &SearchSpace) -> u32 {
        16
    }

    fn additive_indvar_pressure(&self, _: &ir::Type) -> HwPressure {
        HwPressure::zero(self)
    }

    fn multiplicative_indvar_pressure(&self, _: &ir::Type) -> HwPressure {
        HwPressure::zero(self)
    }

    fn thread_rates(&self) -> HwPressure {
        HwPressure::new(1.0, vec![1.0, 1.0, 1.0])
    }

    fn block_rates(&self) -> HwPressure {
        HwPressure::new(1.0, vec![1.0, 1.0, 1.0])
    }

    fn total_rates(&self) -> HwPressure {
        HwPressure::new(1.0, vec![1.0, 1.0, 1.0])
    }

    fn add_block_overhead(
        &self,
        _: model::size::FactorRange,
        _: model::size::FactorRange,
        _: model::size::Range,
        _: &mut HwPressure,
    ) {
    }
}

/// A fake context.
#[derive(Default)]
pub struct Context {
    pub device: Device,
}

impl device::Context for Context {
    fn device(&self) -> &device::Device {
        &self.device
    }

    fn evaluate(&self, _: &codegen::Function, _: device::EvalMode) -> Result<f64, ()> {
        Ok(1.0)
    }

    fn benchmark(&self, _: &codegen::Function, num_samples: usize) -> Vec<f64> {
        vec![1.0; num_samples]
    }

    fn param_as_size(&self, _: &str) -> Option<u32> {
        Some(1)
    }

    fn async_eval<'b, 'c>(
        &self,
        _: usize,
        _: device::EvalMode,
        inner: &(Fn(&mut device::AsyncEvaluator<'b, 'c>) + Sync),
    ) {
        inner(&mut Evaluator {
            phantom: PhantomData,
        });
    }
}

impl device::ArgMap for Context {
    type Array = Array;

    fn bind_scalar<S: ScalarArgument>(&mut self, param: &ir::Parameter, _: S) {
        assert_eq!(param.t, S::t());
    }

    fn bind_array<S: ScalarArgument>(
        &mut self,
        _: &ir::Parameter,
        _: usize,
    ) -> Arc<Self::Array> {
        Arc::new(Array)
    }
}

pub struct Array;

impl ArrayArgument for Array {
    fn read_i8(&self) -> Vec<i8> {
        vec![]
    }

    fn write_i8(&self, _: &[i8]) {}
}

/// A fake asynchronous evaluator.
struct Evaluator<'a, 'b> {
    phantom: PhantomData<(&'a (), &'b ())>,
}

impl<'a, 'b, 'c> device::AsyncEvaluator<'a, 'c> for Evaluator<'a, 'b>
where
    'a: 'b,
    'c: 'b,
{
    fn add_kernel(
        &mut self,
        candidate: Candidate<'a>,
        callback: device::AsyncCallback<'a, 'c>,
    ) {
        // Try to compile the function to check it works.
        codegen::Function::build(&candidate.space);
        callback.call(candidate, 1.0);
    }
}
