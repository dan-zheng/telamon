//! C API wrappers to create Telamon Kernels.
use crate::Device;
use libc;
use num::rational::Ratio;
use std::{self, sync::Arc};
use telamon::ir;
use telamon_utils::*;

pub use telamon::ir::op::Rounding;

use super::error::TelamonStatus;

/// Creates a function signature that must be deallocated with
/// `telamon_ir_signature_free`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_signature_new(
    name: *const libc::c_char,
) -> *const ir::Signature {
    let name = unwrap!(std::ffi::CStr::from_ptr(name).to_str());
    Arc::into_raw(Arc::new(ir::Signature::new(name.to_string())))
}

/// Deallocates a signature created with `telamon_ir_signature_new`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_signature_free(signature: *const ir::Signature) {
    std::mem::drop(Arc::from_raw(signature));
}

/// Returns the parameter at the given position.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_signature_param(
    signature: *const ir::Signature,
    index: usize,
) -> *const ir::Parameter {
    &*(*signature).params[index]
}

/// Adds a scalar parameter to the function signature.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_signature_add_scalar(
    signature: *mut ir::Signature,
    name: *const libc::c_char,
    t: *const ir::Type,
) {
    let name = unwrap!(std::ffi::CStr::from_ptr(name).to_str());
    (*signature).add_scalar(name.to_string(), *t);
}

/// Adds an array parameter to the function signature.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_signature_add_array(
    signature: *mut ir::Signature,
    name: *const libc::c_char,
    element_type: *const ir::Type,
    device: *const Device,
) {
    let name = unwrap!(std::ffi::CStr::from_ptr(name).to_str());
    (*signature).add_array(&*(*device).0, name.to_string(), *element_type)
}

/// Creates an integer type that must be freed with `telamon_ir_type_free`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_type_new_int(num_bits: u16) -> *mut ir::Type {
    Box::into_raw(Box::new(ir::Type::I(num_bits)))
}

/// Creates a floating point type that must be freed with `telamon_ir_type_free`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_type_new_float(num_bits: u16) -> *mut ir::Type {
    Box::into_raw(Box::new(ir::Type::F(num_bits)))
}

/// Frees a type allocated with `telamon_ir_type_new_int` or `telamon_ir_type_new_float`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_type_free(t: *mut ir::Type) {
    std::mem::drop(Box::from_raw(t));
}

/// Opaque type that abstracts away the lifetime parameter of `ir::Function` so that
/// cbindgen generates the bindings.
#[derive(Clone)]
pub struct Function(ir::Function<()>);

impl Into<ir::Function<()>> for Function {
    fn into(self) -> ir::Function<()> {
        self.0
    }
}

/// Creates a function to optimize. The function must be freed with
/// `telamon_ir_function_free`. `signature` and `device` must outlive the function.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_function_new(
    signature: *const ir::Signature,
    device: *const Device,
) -> *mut Function {
    Box::into_raw(Box::new(Function(ir::Function::new(
        Arc::clone(&Arc::from_raw(signature)),
        Arc::clone(&Arc::from_raw((*device).0)),
    ))))
}

/// Frees a function allocated with `telamon_ir_function_new`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_function_free(function: *mut Function) {
    std::mem::drop(Box::from_raw(function));
}

/// Adds an instruction performing the given operator in the given dimensions to the
/// function. Writes the unique identifier of the instruction in `inst_id`. Returns
/// `Ok` except if an error occurs. Takes ownership of the operator
/// but does not keeps any reference to `dimensions`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_function_add_instruction(
    function: *mut Function,
    operator: *mut Operator,
    dimensions: *const ir::DimId,
    num_dimensions: usize,
    inst_id: *mut ir::InstId,
) -> TelamonStatus {
    let dimensions = std::slice::from_raw_parts(dimensions, num_dimensions);
    let dim_set = dimensions.iter().cloned().collect();
    let operator = Box::from_raw(operator).0;
    *inst_id = unwrap_or_exit!((*function).0.add_inst(operator, dim_set));
    TelamonStatus::Ok
}

/// Adds a logical dimension of the given size to the function. In practice, this creates a
/// dimension for each tiling level plus one. Takes ownership of `size` and writes the unique
/// identifier of the logical dimension in `logical_id`. Writes the ids of the dimensions, from the
/// outermost to the innermost, in `dim_ids`. `dim_ids` must be at least of size `num_tiles + 1`.
/// Returns `Ok` except if an error occurs.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_function_add_dimensions(
    function: *mut Function,
    size: *mut Size,
    tile_sizes: *const u32,
    num_tiles: usize,
    logical_id: *mut ir::LogicalDimId,
    dim_ids: *mut ir::DimId,
) -> TelamonStatus {
    let tile_sizes = std::slice::from_raw_parts(tile_sizes, num_tiles);
    let tiling_factors = vec![tile_sizes.iter().product::<u32>()];
    let tile_sizes = tile_sizes.iter().map(|&s| VecSet::new(vec![s])).collect();
    let size = Box::from_raw(size).0;
    let (ldim, dims) = unwrap_or_exit!((*function).0.add_logical_dim(
        size,
        tiling_factors.into(),
        tile_sizes
    ));
    *logical_id = ldim;
    std::ptr::copy_nonoverlapping(dims.as_ptr(), dim_ids, num_tiles + 1);
    TelamonStatus::Ok
}

/// Opaque type that abstracts away the lifetime parameter of `ir::Size` so cbindgen
/// can generate bindings.
pub struct Size(ir::Size);

/// Create a size equal to:
/// ```
/// const_factor * param_factors[0] * .. * param_factors[num_params-1]
/// ```
/// The size must be freed calling `telamon_ir_size_free` or passed to a function that
/// takes its ownership.
// TODO(bclement): Adapt to Arc<Parameters>
/*
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_size_new(
    const_factor: u32,
    param_factors: *const *const ir::Parameter,
    num_params: usize,
    max_val: u32,
) -> *mut Size {
    let parameters = std::slice::from_raw_parts(param_factors, num_params)
        .iter()
        .map(|&ptr| Arc::new(*ptr))
        .collect();
    let size = ir::Size::new(const_factor, parameters, max_val);
    Box::into_raw(Box::new(Size(size)))
}*/

/// Frees a size allocated with `telamon_ir_size_new`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_size_free(size: *mut Size) {
    std::mem::drop(Box::from_raw(size));
}

/// Opaque type that abstracts away the lifetime parameter of `ir::SizeiPartial` so
/// cbindgen can generate bindings.
pub struct PartialSize(ir::PartialSize);

/// Converts an `ir::Size` into an `ir::PartialSize`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_size_into_partial(
    size: *mut Size,
) -> *mut PartialSize {
    let size = Box::from_raw(size).0.into();
    Box::into_raw(Box::new(PartialSize(size)))
}

/// Returns the size of a dimension.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_dimension_size(
    function: *const Function,
    dim: ir::DimId,
) -> *mut PartialSize {
    let size = (*function).0.dim(dim).size().clone();
    Box::into_raw(Box::new(PartialSize(size)))
}

/// Multiplies `lhs` by `rhs`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_size_mul(
    lhs: *mut PartialSize,
    rhs: *const PartialSize,
) {
    (*lhs).0 *= &(*rhs).0;
}

/// Opaque type that abstracts away the lifetime parameter of `ir::Operand` so that
/// cbindgen can generate bindings.
pub struct Operand(ir::Operand<()>);

/// Create a constant integer operand. The provided type must be an integer type.
/// Returns `null` if an error is encountered.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operand_new_int(
    t: *const ir::Type,
    value: libc::int64_t,
) -> *mut Operand {
    unwrap_or_exit!(ir::TypeError::check_integer(*t), null);
    let type_len = unwrap!((*t).len_byte()) as u16;
    let operand = ir::Operand::new_int(value.into(), 8 * type_len);
    Box::into_raw(Box::new(Operand(operand)))
}

/// Creates a constant floating point operand. The provided type must be a float type.
/// Returns `null` if an error is encountered.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operand_new_float(
    t: *const ir::Type,
    value: libc::c_double,
) -> *mut Operand {
    unwrap_or_exit!(ir::TypeError::check_float(*t), null);
    let type_len = unwrap!((*t).len_byte()) as u16;
    let value = unwrap!(Ratio::from_float(value));
    let operand = ir::Operand::new_float(value, type_len);
    Box::into_raw(Box::new(Operand(operand)))
}

/// Creates an operand that fetches the value of a parameter. The created operand holds
/// a reference to `parameter`.
// TODO(bclement): Adapt to Arc<Parameter>
/*
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operand_new_parameter(
    parameter: *const ir::Parameter,
) -> *mut Operand {
    let operand = ir::Operand::Param(Arc::new(*parameter));
    Box::into_raw(Box::new(Operand(operand)))
}
*/

/// Creates an operand that returns the current index on a dimension.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operand_new_index(dim: ir::DimId) -> *mut Operand {
    let operand = ir::Operand::Index(dim);
    Box::into_raw(Box::new(Operand(operand)))
}

/// Creates an operand that references the value of an instruction. The value of the
/// instruction is transmitted point-to-point between the source dimensions (`src_dims`,
/// in which the instruction is produced) and destination dimensions (`dst_dims`, in which
/// the operand is used). `num_mapped_dims` indicates the number of dimensions in
/// `src_dims` and in `dst_dims`. If `allow_tmp_mem` is non-zero, Telamon can allocate
/// memory to transfer data between the two loop nests. Otherwise, it makes sure the data
/// can be stored in registers (for example by fusing or unrolling loops).
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operand_new_inst(
    function: *const Function,
    inst: ir::InstId,
    src_dims: *const ir::DimId,
    dst_dims: *const ir::DimId,
    num_mapped_dims: usize,
    allow_tmp_mem: libc::c_int,
) -> *mut Operand {
    let inst = (*function).0.inst(inst);
    let dim_map = dim_map_from_arrays(src_dims, dst_dims, num_mapped_dims);
    let dim_map_scope = if allow_tmp_mem == 0 {
        ir::DimMapScope::Thread
    } else {
        ir::DimMapScope::Global(())
    };
    let operand = ir::Operand::new_inst(inst, dim_map, dim_map_scope);
    Box::into_raw(Box::new(Operand(operand)))
}

/// Creates an operand that take the value of `init_inst` the first time is is encountered
/// and then reuse the value produced by the instruction using the operand, effectivelly
/// creating a reduction. The value is is transmitted point-to-point between the source
/// dimensions (`src_dims`, in which `init_inst` is produced) and destination dimensions
/// (`dst_dims`, in which the operand is used). `num_mapped_dims` indicates the number of
/// dimensions in `src_dims` and in `dst_dims`. `reduction_dims` indicates on which
/// dimensions the reduction occurs: values are not reused accross other dimensions.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operand_new_reduction(
    function: *const Function,
    init_inst: ir::InstId,
    src_dims: *const ir::DimId,
    dst_dims: *const ir::DimId,
    num_mapped_dims: usize,
    reduction_dims: *const ir::DimId,
    num_reduction_dims: usize,
) -> *mut Operand {
    let init = (*function).0.inst(init_inst);
    let reduction_dims =
        std::slice::from_raw_parts(reduction_dims, num_reduction_dims).to_vec();
    let dim_map = dim_map_from_arrays(src_dims, dst_dims, num_mapped_dims);
    let operand = ir::Operand::new_reduce(init, dim_map, reduction_dims);
    Box::into_raw(Box::new(Operand(operand)))
}

/// Helper function that creates a `DimMap` from C arrays of dimensions. Does not holds
/// references after the function exits.
unsafe fn dim_map_from_arrays(
    src_dims: *const ir::DimId,
    dst_dims: *const ir::DimId,
    num_mapped_dims: usize,
) -> ir::DimMap {
    let src_dims = std::slice::from_raw_parts(src_dims, num_mapped_dims);
    let dst_dims = std::slice::from_raw_parts(dst_dims, num_mapped_dims);
    let dims = src_dims.iter().cloned().zip(dst_dims.iter().cloned());
    ir::DimMap::new(dims)
}

/// Opaque type that abstracts away the lifetime parameter of `ir::Operator` so that
/// cbindgen can generate bindings.
pub struct Operator(ir::Operator<()>);

/// Creates a `mov` operator. Takes ownership of `operand`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_mov(
    operand: *mut Operand,
) -> *mut Operator {
    let operator = ir::Operator::UnaryOp(ir::UnaryOp::Mov, Box::from_raw(operand).0);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Creates a binary operator. Takes ownership of the operands.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_binop(
    binop: ir::BinOp,
    lhs: *mut Operand,
    rhs: *mut Operand,
    rounding: ir::op::Rounding,
) -> *mut Operator {
    let lhs = Box::from_raw(lhs).0;
    let rhs = Box::from_raw(rhs).0;
    let operator = ir::Operator::BinOp(binop, lhs, rhs, rounding);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Creates a `mul` operator. The return type can either be the operands type or, if the
/// multplication operates on integers, a type twice the size of the input. Takes
/// ownership of both `lhs` and `rhs`. No references to `return_type` is hold after the
/// function returns.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_mul(
    lhs: *mut Operand,
    rhs: *mut Operand,
    rounding: ir::op::Rounding,
    return_type: *const ir::Type,
) -> *mut Operator {
    let lhs = Box::from_raw(lhs).0;
    let rhs = Box::from_raw(rhs).0;
    let operator = ir::Operator::Mul(lhs, rhs, rounding, *return_type);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Creates a `mad` operator, that computes `mul_lhs * mul_rhs + add_rhs`. If the operator
/// operates on integer, the type of `add_rhs` can either be the type of both `mul_lhs`
/// and `mul_rhs` or an integer type having twice the size of the multiplied types. Takes
/// ownership of `mul_lhs`, `mul_rhs` and `add_rhs`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_mad(
    mul_lhs: *mut Operand,
    mul_rhs: *mut Operand,
    add_rhs: *mut Operand,
    rounding: ir::op::Rounding,
) -> *mut Operator {
    let mul_lhs = Box::from_raw(mul_lhs).0;
    let mul_rhs = Box::from_raw(mul_rhs).0;
    let add_rhs = Box::from_raw(add_rhs).0;
    let operator = ir::Operator::Mad(mul_lhs, mul_rhs, add_rhs, rounding);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Creates a `cast` operator. Takes ownership of `operand`. No reference to `return_type`
/// is hold after the function returns.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_cast(
    operand: *mut Operand,
    return_type: *const ir::Type,
) -> *mut Operator {
    let operand = Box::from_raw(operand).0;
    let operator = ir::Operator::UnaryOp(ir::UnaryOp::Cast(*return_type), operand);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Creates an operator that loads a tensor stored in memory. Takes the ownership of
/// `base_address` and creates copies of `strided_dims`, `strides` and `loaded_type`.
/// This function also adds the necessary address computation code to `function`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_tensor_load(
    function: *mut Function,
    array_id: *const ir::MemId,
    base_address: *mut Operand,
    strided_dims: *const ir::DimId,
    strides: *const PartialSize,
    num_strided_dims: usize,
    loaded_type: *const ir::Type,
) -> *mut Operator {
    let tensor_access = tensor_access(
        function,
        array_id,
        base_address,
        strided_dims,
        strides,
        num_strided_dims,
    );
    let (address, access_pattern) = unwrap_or_exit!(tensor_access, null);
    let operator = ir::Operator::Ld(*loaded_type, address, access_pattern);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Creates an operator that stores a tensor in memory. Takes the ownership of
/// `base_address` and `value` and creates copies of `strided_dims`, `strides` and
/// `loaded_type`. This function also adds the necessary address computation code to
/// `function`.
#[no_mangle]
pub unsafe extern "C" fn telamon_ir_operator_new_tensor_store(
    function: *mut Function,
    array_id: *const ir::MemId,
    base_address: *mut Operand,
    strided_dims: *const ir::DimId,
    strides: *const PartialSize,
    num_strided_dims: usize,
    value: *mut Operand,
) -> *mut Operator {
    let tensor_access = tensor_access(
        function,
        array_id,
        base_address,
        strided_dims,
        strides,
        num_strided_dims,
    );
    let (address, access_pattern) = unwrap_or_exit!(tensor_access, null);
    let value = Box::from_raw(value).0;
    let operator = ir::Operator::St(address, value, true, access_pattern);
    Box::into_raw(Box::new(Operator(operator)))
}

/// Helper function that generates the address and the access pattern of a tensor
/// memory access. Takes the ownership of `base_adress`, and creates copies of
/// `strided_dims` and `strides`.
unsafe fn tensor_access(
    function: *mut Function,
    array_id: *const ir::MemId,
    base_address: *mut Operand,
    strided_dims: *const ir::DimId,
    strides: *const PartialSize,
    num_strided_dims: usize,
) -> Result<(ir::Operand<()>, ir::AccessPattern), ir::Error> {
    let base_address = Box::from_raw(base_address).0;
    let ptr_type = base_address.t();
    let strided_dims = std::slice::from_raw_parts(strided_dims, num_strided_dims);
    let strides = std::slice::from_raw_parts(strides, num_strided_dims);
    let address = if strided_dims.is_empty() {
        base_address
    } else {
        let dims = (0..num_strided_dims)
            .map(|i| (strided_dims[i], strides[i].0.clone()))
            .collect();
        let ind_var = ir::InductionVar::new(dims, base_address)?;
        let ind_var_id = (*function).0.add_ind_var(ind_var);
        ir::Operand::InductionVar(ind_var_id, ptr_type)
    };
    let dims = (0..num_strided_dims)
        .map(|i| (strided_dims[i], strides[i].0.clone()))
        .collect();
    let access_pattern = ir::AccessPattern::Tensor {
        mem_id: if array_id.is_null() {
            None
        } else {
            Some(*array_id)
        },
        dims,
    };
    Ok((address, access_pattern))
}
