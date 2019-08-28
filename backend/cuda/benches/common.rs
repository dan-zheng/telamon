//! Defines a matrix-matrix multiply kernel.
#![allow(dead_code)]
#![cfg(feature = "real_gpu")]

use lazy_static::lazy_static;
use rand::Rng;
use std::sync::Arc;
use telamon::explorer::choice::ActionEx;
use telamon::search_space::*;
use telamon::{explorer, helper, ir};
use telamon_cuda as cuda;

lazy_static! {
    /// A fake GPU description, used only to know which candidates are valid.
    static ref DEVICE: Arc<cuda::Gpu> = Arc::new(cuda::Gpu::dummy());

    static ref MM_SIGNATURE: MMSig = MMSig::signature();
    pub static ref MM: SearchSpace = MM_SIGNATURE.build_body();
}

const DATA_TYPE: ir::Type = ir::Type::F(32);

/// Stores the signature and the external arrays IDs for matrix-matrix multiplication.
struct MMSig {
    signature: Arc<ir::Signature>,
}

impl MMSig {
    fn signature() -> Self {
        let mut signature = ir::Signature::new("mm".to_string());
        signature.add_scalar("m".to_string(), ir::Type::I(32));
        signature.add_scalar("n".to_string(), ir::Type::I(32));
        signature.add_scalar("k".to_string(), ir::Type::I(32));
        signature.add_array(&**DEVICE, "a".to_string(), DATA_TYPE);
        signature.add_array(&**DEVICE, "b".to_string(), DATA_TYPE);
        signature.add_array(&**DEVICE, "c".to_string(), DATA_TYPE);
        MMSig {
            signature: Arc::new(signature),
        }
    }

    fn build_body(&self) -> SearchSpace {
        let mut builder = helper::Builder::new(
            Arc::clone(&self.signature),
            Arc::<cuda::Gpu>::clone(&DEVICE),
        );
        let m_size = builder.param_size("m", 32);
        let n_size = builder.param_size("n", 32);
        let k_size = builder.param_size("k", 32);

        let ld_a_m = builder.open_tiled_dim(m_size, [16, 4][..].into());
        let ld_a_k = builder.open_tiled_dim(k_size.clone(), [16][..].into());
        let (ptr, pattern) =
            builder.tensor_access(&"a", None, DATA_TYPE, &[&ld_a_m, &ld_a_k]);
        let ld_a = builder.ld_nc(DATA_TYPE, &ptr, pattern);
        builder.close_dim(&ld_a_m);
        builder.close_dim(&ld_a_k);

        let ld_b_k = builder.open_tiled_dim(k_size, [16][..].into());
        let ld_b_n = builder.open_tiled_dim(n_size, [16, 4][..].into());
        let (ptr, pattern) =
            builder.tensor_access(&"b", None, DATA_TYPE, &[&ld_b_k, &ld_b_n]);
        let ld_b = builder.ld_nc(DATA_TYPE, &ptr, pattern);
        builder.close_dim(&ld_b_k);
        builder.close_dim(&ld_b_n);

        let init_m = builder.open_mapped_dim(&ld_a_m);
        let init_n = builder.open_mapped_dim(&ld_b_n);
        let init = builder.mov(&0f32);

        let acc_m = builder.open_mapped_dim(&init_m);
        let acc_n = builder.open_mapped_dim(&init_n);
        let acc_k = builder.open_mapped_dim(&ld_b_k);
        let a_op = builder.dim_map(
            ld_a,
            &[(&ld_a_m, &acc_m), (&ld_a_k, &acc_k)],
            ir::DimMapScope::Global(()),
        );
        let b_op = builder.dim_map(
            ld_b,
            &[(&ld_b_k, &acc_k), (&ld_b_n, &acc_n)],
            ir::DimMapScope::Global(()),
        );
        let acc = builder.mad(&a_op, &b_op, &helper::Reduce(init));

        builder.close_dim(&acc_k);
        let st_m = builder.open_mapped_dim(&acc_m);
        let st_n = builder.open_mapped_dim(&acc_n);
        let (ptr, pattern) =
            builder.tensor_access(&"c", None, DATA_TYPE, &[&st_m, &st_n]);
        let st = builder.st(&ptr, &acc, pattern);
        // order for correctness.
        builder.order(&st, &acc_k, Order::AFTER);
        builder.get()
    }
}

/// Descends in the search tree without saving the candidates.
#[allow(clippy::let_and_return)]
pub fn descend_without_copies(mut space: SearchSpace) {
    while let Some(mut choice) = {
        let choice = explorer::choice::default_list(&space).next();
        choice
    } {
        let id = rand::thread_rng().gen_range(0, choice.len());
        let res = match choice.swap_remove(id) {
            ActionEx::Action(action) => space.apply_decisions(vec![action]),
            ActionEx::LowerLayout {
                mem,
                ref st_dims,
                ref ld_dims,
            } => space.lower_layout(mem, st_dims, ld_dims),
        };
        if res.is_err() {
            return;
        }
    }
}

/// Descends in the search tree and returns the candidates encountered.
#[allow(clippy::let_and_return)]
pub fn descend_with_copies(mut space: SearchSpace) -> Vec<SearchSpace> {
    let mut spaces = vec![];
    while let Some(mut choice) = {
        let choice = explorer::choice::default_list(&space).next();
        choice
    } {
        let id = rand::thread_rng().gen_range(0, choice.len());
        let res = match choice.swap_remove(id) {
            ActionEx::Action(action) => space.apply_decisions(vec![action]),
            ActionEx::LowerLayout {
                mem,
                ref st_dims,
                ref ld_dims,
            } => space.lower_layout(mem, st_dims, ld_dims),
        };
        if res.is_err() {
            return spaces;
        }
        spaces.push(space.clone());
    }
    spaces
}
