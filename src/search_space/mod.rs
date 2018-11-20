//! Search space datastructures and constraint propagation.
use ir;

generated_file!(choices);
mod operand;
mod variable;

pub use self::choices::{
    Action, Bool, Choice, DimKind, Domain, DomainStore, HalfRange, InstFlag,
    IsInstantiated, MemorySpace, NumDomain, NumSet, NumericSet, Order, ThreadMapping,
    VarDefMode,
};

use self::choices::{apply_action, init_domain, DomainDiff};
use std::sync::Arc;

/// A partially specified implementation.
#[derive(Clone)]
pub struct SearchSpace<'a> {
    ir_instance: Arc<ir::Function<'a>>,
    domain: DomainStore,
}

impl<'a> SearchSpace<'a> {
    /// Creates a new `SearchSpace` for the given `ir_instance`.
    pub fn new(
        ir_instance: ir::Function<'a, ()>,
        mut actions: Vec<Action>,
    ) -> Result<Self, ()> {
        // Pre-allocate IDs for future lowerings.
        let mut ir_instance = ir_instance.freeze();

        let mut domain = DomainStore::new(&ir_instance);
        // Enforce invariants.
        for inst in ir_instance.insts() {
            actions.extend(operand::inst_invariants(&ir_instance, inst));
        }
        let mut unused_diff = DomainDiff::default();
        for action in actions {
            apply_action(action, &mut domain, &mut unused_diff)?;
        }
        let actions = init_domain(&mut domain, &mut ir_instance)?;
        let mut space = SearchSpace {
            ir_instance: Arc::new(ir_instance),
            domain,
        };
        space.apply_decisions(actions)?;
        Ok(space)
    }

    /// Returns the underlying ir instance.
    pub fn ir_instance(&self) -> &ir::Function<'a> {
        &self.ir_instance
    }

    /// Returns the domain of choices.
    pub fn domain(&self) -> &DomainStore {
        &self.domain
    }

    /// Allows rewritting the domain.
    pub fn domain_mut(&mut self) -> &mut DomainStore {
        &mut self.domain
    }

    /// Applies a list of decisions to the domain and propagate constraints.
    pub fn apply_decisions(&mut self, actions: Vec<Action>) -> Result<(), ()> {
        choices::apply_decisions(actions, &mut self.ir_instance, &mut self.domain)
    }
}

/// Update the domain after a lowering.
fn process_lowering(
    ir_instance: &mut ir::Function,
    domain: &mut DomainStore,
    new_objs: &ir::NewObjs,
    diff: &mut DomainDiff,
) -> Result<Vec<Action>, ()> {
    let mut actions = Vec::new();
    debug!("adding objects {:?}", new_objs);
    domain.alloc(ir_instance, new_objs);
    actions.extend(choices::init_domain_partial(
        domain,
        ir_instance,
        new_objs,
        diff,
    )?);
    // Enforce invariants and call manual triggers.
    for &inst in &new_objs.instructions {
        actions.extend(operand::inst_invariants(
            ir_instance,
            ir_instance.inst(inst),
        ));
    }
    // Manually restrict the possible ranks until we find why this is not automatically
    // performed by Telamon-Gen
    // TODO(ulysse): fix telamon_gen.
    for &mem in &new_objs.memory_vars {
        let num_mem_dims = domain.get_num_mem_dims(mem);
        for &id in ir_instance.variable(mem).layout() {
            let universe = unwrap!(ir_instance.layout_dimension(id).possible_ranks());
            let ranks = NumericSet::new_leq(universe, num_mem_dims, &());
            actions.push(Action::Rank(id, ranks));
        }
    }
    Ok(actions)
}

/// Adds a iteration dimension to a basic block.
fn add_iteration_dim(
    ir_instance: &mut ir::Function,
    inst: ir::InstId,
    dim: ir::DimId,
) -> ir::NewObjs {
    debug!("set {:?} as iteration dim of inst {:?}", dim, inst);
    let mut new_objs = ir::NewObjs::default();
    if ir_instance.set_iteration_dim(inst, dim) {
        new_objs.add_iteration_dim(inst, dim);
    }
    new_objs
}

/// Adds a dimension to the list of thread dimensions.
fn add_thread_dim(ir_instance: &mut ir::Function, dim: ir::DimId) -> ir::NewObjs {
    debug!("set {:?} as a thread dimension", dim);
    let mut new_objs = ir::NewObjs::default();
    if ir_instance.add_thread_dim(dim) {
        new_objs.add_thread_dim(dim);
    }
    new_objs
}

/// Returns the memory space accessed by an access pattern.
pub fn array_memory_space(array: ir::ArrayId, space: &SearchSpace) -> MemorySpace {
    match array {
        ir::ArrayId::External => MemorySpace::GLOBAL,
        ir::ArrayId::Static(id) => match space.ir_instance().mem_block(id).space {
            ir::MemorySpace::Global => MemorySpace::GLOBAL,
            ir::MemorySpace::Shared => MemorySpace::SHARED,
        },
        ir::ArrayId::Variable(var) => space.domain().get_memory_space(var),
    }
}
