//! Defines the CPU target.
// TODO(mppa): improve the IR.
// - check privatisation between threads is correct
// - allow multiple threads groups in the same cluster, to overlap transfers
// - blocking async transfers
// - events + retiming
mod context;
mod mppa;
mod printer;
mod telajax;

pub use self::context::Context;
pub use self::mppa::Mppa;
pub use self::printer::MppaPrinter;

use crate::codegen;
use crate::ir;
use itertools::Itertools;
use num::bigint::BigInt;
use num::rational::Ratio;
use num::ToPrimitive;
use utils::*;

#[derive(Default)]
pub struct Namer {
    num_var: HashMap<ir::Type, usize>,
    num_sizes: usize,
}

impl Namer {
    pub fn gen_prefix(t: ir::Type) -> &'static str {
        match t {
            ir::Type::I(1) => "p",
            ir::Type::I(8) => "c",
            ir::Type::I(16) => "s",
            ir::Type::I(32) => "r",
            ir::Type::I(64) => "rd",
            ir::Type::F(16) => "h",
            ir::Type::F(32) => "f",
            ir::Type::F(64) => "d",
            ir::Type::Ptr => "ptr",
            _ => panic!("invalid CPU type"),
        }
    }

    pub fn get_string(t: ir::Type) -> &'static str {
        match t {
            ir::Type::Ptr => "uintptr_t",
            ir::Type::F(32) => "float",
            ir::Type::F(64) => "double",
            ir::Type::I(1) => "uint8_t",
            ir::Type::I(8) => "uint8_t",
            ir::Type::I(16) => "uint16_t",
            ir::Type::I(32) => "uint32_t",
            ir::Type::I(64) => "uint64_t",
            _ => panic!("invalid CPU type"),
        }
    }
}

impl codegen::Namer for Namer {
    fn name(&mut self, t: ir::Type) -> String {
        let prefix = Namer::gen_prefix(t);
        let entry = self.num_var.entry(t).or_insert(0);
        let name = format!("{}{}", prefix, *entry);
        *entry += 1;
        name
    }

    fn name_float(&self, val: &Ratio<BigInt>, len: u16) -> String {
        assert!(len <= 64);
        let f = unwrap!(val.numer().to_f64()) / unwrap!(val.denom().to_f64());
        format!("{:.5e}", f)
    }

    fn name_int(&self, val: &BigInt, len: u16) -> String {
        assert!(len <= 64);
        format!("{}", unwrap!(val.to_i64()))
    }

    fn name_param(&mut self, p: codegen::ParamValKey) -> String {
        match p {
            codegen::ParamValKey::External(p) => p.name.clone(),
            codegen::ParamValKey::GlobalMem(mem) => format!("_gbl_mem_{}", mem.0),
            codegen::ParamValKey::Size(_) => {
                self.num_sizes += 1;
                format!("_size_{}", self.num_sizes - 1)
            }
        }
    }
}
