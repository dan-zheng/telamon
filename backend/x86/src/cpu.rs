//! Describes CUDA-enabled GPUs.
use telamon::codegen::Function;
use telamon::device;
use telamon::ir::{self, Type};
use telamon::model::{self, HwPressure};
use telamon::search_space::*;

use std::io::Write;
use utils::*;

/// Represents CUDA GPUs.
#[derive(Clone)]
pub struct Cpu {
    /// The name of the CPU.
    pub name: String,
}

impl Cpu {
    pub fn dummy_cpu() -> Self {
        Cpu {
            name: String::from("x86"),
        }
    }
}

impl device::Device for Cpu {
    fn print(&self, _fun: &Function, out: &mut Write) {
        unwrap!(write!(out, "Basic CPU"));
    }

    fn check_type(&self, t: Type) -> Result<(), ir::TypeError> {
        match t {
            Type::I(i) | Type::F(i) if i == 32 || i == 64 => Ok(()),
            Type::I(i) if i == 1 || i == 8 || i == 16 => Ok(()),
            Type::PtrTo(_) => Ok(()),
            t => Err(ir::TypeError::InvalidType { t }),
        }
    }

    // block dimensions do not make sense on cpu
    fn max_block_dims(&self) -> u32 {
        0
    }

    fn max_inner_block_size(&self) -> u32 {
        1
    }

    fn max_threads(&self) -> u32 {
        8
    }

    fn max_unrolling(&self) -> u32 {
        512
    }

    fn can_vectorize(&self, _: &ir::Dimension, _: &ir::Operator) -> bool {
        false
    }

    fn max_vectorization(&self, _: &ir::Operator) -> [u32; 2] {
        [1, 1]
    }

    fn has_vector_registers(&self) -> bool {
        false
    }

    fn shared_mem(&self) -> u32 {
        0
    }

    fn pointer_type(&self, _: MemSpace) -> ir::Type {
        // Use 0 as a dummy memory ID.
        ir::Type::PtrTo(ir::MemId(0))
    }

    fn supported_mem_flags(&self, op: &ir::Operator) -> InstFlag {
        match op {
            ir::Operator::Ld(..)
            | ir::Operator::St(..)
            | ir::Operator::TmpLd(..)
            | ir::Operator::TmpSt(..) => InstFlag::BLOCK_COHERENT,
            _ => panic!("not a memory operation"),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn add_block_overhead(
        &self,
        _: model::size::FactorRange,
        _: model::size::FactorRange,
        _: model::size::Range,
        _: &mut HwPressure,
    ) {
    }

    fn lower_type(&self, t: ir::Type, _space: &SearchSpace) -> Option<ir::Type> {
        Some(t)
    }

    fn hw_pressure(
        &self,
        _: &SearchSpace,
        _: &FnvHashMap<ir::DimId, model::size::Range>,
        _: &FnvHashMap<ir::StmtId, model::Nesting>,
        _: &ir::Statement,
        _: &device::Context,
    ) -> model::HwPressure {
        // TODO(model): implement model
        model::HwPressure::zero(self)
    }

    fn loop_iter_pressure(&self, _kind: DimKind) -> (HwPressure, HwPressure) {
        //TODO(model): implement minimal model
        (model::HwPressure::zero(self), model::HwPressure::zero(self))
    }

    fn thread_rates(&self) -> HwPressure {
        //TODO(model): implement minimal model
        model::HwPressure::new(1.0, vec![])
    }

    fn block_rates(&self) -> HwPressure {
        //TODO(model): implement minimal model
        model::HwPressure::new(1.0, vec![])
    }

    fn total_rates(&self) -> HwPressure {
        //TODO(model): implement minimal model
        model::HwPressure::new(1.0, vec![])
    }

    fn bottlenecks(&self) -> &[&'static str] {
        &[]
    }

    fn block_parallelism(&self, _space: &SearchSpace) -> u32 {
        1
    }

    fn additive_indvar_pressure(&self, _t: &ir::Type) -> HwPressure {
        //TODO(model): implement minimal model
        model::HwPressure::zero(self)
    }

    fn multiplicative_indvar_pressure(&self, _t: &ir::Type) -> HwPressure {
        //TODO(model): implement minimal model
        model::HwPressure::zero(self)
    }
}
