//! `DimMap` and layout lowering.
use crate::ir;
use crate::search_space::choices::dim_kind;
use crate::search_space::operand;
use crate::search_space::{Action, DimKind, DomainStore, MemSpace, Order};
use itertools::Itertools;
use log::debug;

/// Lowers a layout
pub fn lower_layout(
    fun: &mut ir::Function,
    mem: ir::MemId,
    st_dims: Vec<ir::DimId>,
    ld_dims: Vec<ir::DimId>,
    domain: &DomainStore,
) -> Result<Vec<Action>, ()> {
    debug!("lower_layout({:?}) triggered", mem);
    let mut actions = Vec::new();
    // TODO(automate): vectorization disabled -> express as an additional constraint
    // We need to manually set every dimension except the innermost as non-vectorizable
    // because temporary loads and stores do not restrict vectorization automatically
    // since they are not aware of the access pattern. The constraint propagation is not
    // aware we just specified the access pattern and so doesn't re-run the constraints.
    for (&st_dim, &ld_dim) in st_dims.iter().rev().zip_eq(ld_dims.iter().rev()).skip(1) {
        let not_vec = !DimKind::VECTOR;
        actions.extend(dim_kind::restrict_delayed(st_dim, fun, domain, not_vec)?);
        actions.extend(dim_kind::restrict_delayed(ld_dim, fun, domain, not_vec)?);
    }
    fun.lower_layout(mem, st_dims, ld_dims);
    for &inst_id in fun.mem_block(mem.into()).uses() {
        let inst = fun.inst(inst_id);
        actions.extend(operand::inst_invariants(fun, inst));
    }
    Ok(actions)
}

/// Lowers a `DimMap`.
fn lower_dim_map(
    fun: &mut ir::Function,
    inst: ir::InstId,
    operand: usize,
    new_objs: &mut ir::NewObjs,
) -> Result<Vec<Action>, ()> {
    debug!("lower_dim_map({:?}, {}) triggered", inst, operand);
    let lowered_dim_map = fun.lower_dim_map(inst, operand)?;
    let mut actions = Vec::new();
    // Order the store and load loop nests.
    for (src, dst) in lowered_dim_map.mem_dimensions() {
        actions.push(Action::Order(
            src.into(),
            dst.into(),
            Order::BEFORE | Order::MERGED,
        ));
    }
    // FIXME: allow global memory
    actions.push(Action::MemSpace(
        lowered_dim_map.mem.into(),
        MemSpace::SHARED,
    ));
    //actions.push(Action::InstFlag(st, InstFlag::COHERENT));
    //actions.push(Action::InstFlag(ld, InstFlag::COHERENT));
    let store = lowered_dim_map.store;
    actions.push(Action::Order(
        store.into(),
        lowered_dim_map.load.into(),
        Order::BEFORE,
    ));
    let operand = fun.inst(inst).operands()[operand];
    // The invariants for the load and store instructions, including the ones implied by the
    // mapping between dimensions, are enforced by `search_space::process_lowering`.
    actions.extend(operand::invariants(fun, operand, inst.into()));
    lowered_dim_map.register_new_objs(fun, new_objs);
    debug!("lower_dim_map actions: {:?}", actions);
    Ok(actions)
}

/// Trigger to call when two dimensions are not mapped.
pub fn dim_not_mapped(
    lhs: ir::DimId,
    rhs: ir::DimId,
    fun: &mut ir::Function,
) -> Result<(ir::NewObjs, Vec<Action>), ()> {
    debug!("dim_not_mapped({:?}, {:?}) triggered", lhs, rhs);
    let to_lower = fun
        .insts()
        .flat_map(|inst| {
            inst.dim_maps_to_lower(lhs, rhs)
                .into_iter()
                .map(move |op_id| (inst.id(), op_id))
        })
        .collect_vec();
    let mut new_objs = ir::NewObjs::default();
    let mut actions = Vec::new();
    for (inst, operand) in to_lower {
        actions.extend(lower_dim_map(fun, inst, operand, &mut new_objs)?);
    }
    Ok((new_objs, actions))
}

/// Trigger to call when two dimensions are not merged.
pub fn dim_not_merged(
    lhs: ir::DimId,
    rhs: ir::DimId,
    fun: &mut ir::Function,
) -> Result<(ir::NewObjs, Vec<Action>), ()> {
    debug!("dim_not_merged({:?}, {:?}) triggered", lhs, rhs);
    fun.dim_not_merged(lhs, rhs);
    // TODO(cc_perf): avoid creating a 'NewObjs' object.
    Ok(Default::default())
}
