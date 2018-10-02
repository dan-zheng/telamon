use ir;
use utils::*;

/// Unique identifier for `InductionVar`
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndVarId(pub u32);

/// A multidimentional induction variable. No dimension should appear twice in
/// dims.
#[derive(Clone, Debug)]
pub struct InductionVar<'a> {
    dims: Vec<(ir::dim::Id, ir::Size<'a>)>,
    base: ir::Operand<'a>,
}

impl<'a> InductionVar<'a> {
    /// Creates a new induction var. Size represents the increment over each
    /// diemnsion taken independenly.
    pub fn new(dims: Vec<(ir::dim::Id, ir::Size<'a>)>, base: ir::Operand<'a>) -> Self {
        assert!(base.t().is_integer());
        // Assert dimensions are unique.
        let mut dim_ids = HashSet::default();
        for &(id, _) in &dims {
            assert!(dim_ids.insert(id));
        }
        match base {
            ir::Operand::Reduce(..) => {
                panic!("induction variables cannot perform reductions")
            }
            ir::Operand::Inst(.., ir::DimMapScope::Global) =>
            // TODO(search_space): allow dim map lowering for induction variables
            {
                unimplemented!(
                    "dim map lowering for induction vars is not implemented yet"
                )
            }
            _ => (),
        }
        InductionVar { dims, base }
    }

    /// Renames a dimension.
    pub fn merge_dims(&mut self, lhs: ir::dim::Id, rhs: ir::dim::Id) {
        self.base.merge_dims(lhs, rhs);
    }

    /// Returns the base operand of the induction variable.
    pub fn base(&self) -> &ir::Operand<'a> { &self.base }

    /// Returns the list of induction dimensions along with the corresponding
    /// increments.
    pub fn dims(&self) -> &[(ir::dim::Id, ir::Size<'a>)] { &self.dims }
}
