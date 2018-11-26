//! Abstracts kernels so we can build generic methods to test them.
use itertools::Itertools;
use num_cpus;
use rayon::prelude::*;
use statistics;
use std;
use std::borrow::Cow;
use std::marker::PhantomData;
use std::sync::{atomic, Mutex};
use telamon::explorer::{local_selection, Candidate};
use telamon::helper::SignatureBuilder;
use telamon::model::Bound;
use telamon::{codegen, device, explorer, ir};
use utils::*;

/// Ignore candidates with a too big bound in tests.
const CUT: f64 = 2e8f64;
/// Maximal number of deadends to accept before failing.
// TODO(cleanup): tune MAX_DEADEND_RATIO
//const MAX_DEADEND_RATIO: usize = 20;
const MAX_DEADEND_RATIO: f32 = 0.95;

/// A kernel that can be compiled, benchmarked and used for correctness tests.
pub trait Kernel<'a>: Sized {
    /// The input parameters of the kernel.
    type Parameters: Clone;
    /// The values to expect as output.
    type ExpectedOutput;

    /// The name of the function computed by the kernel.
    fn name() -> &'static str;

    /// Builds the signature of the kernel in the builder and returns an object that
    /// stores enough information to later build the kernel body and check its result.
    /// The `is_generic` flag indicates if th sizes should be instantiated.
    fn build_signature<AM>(
        parameters: Self::Parameters,
        builder: &mut SignatureBuilder<AM>,
    ) -> Self
    where
        AM: device::ArgMap + device::Context + 'a;

    /// Builder the kernel body in the given builder. This builder should be based on the
    /// signature created by `build_signature`.
    fn build_body<'b>(
        &self,
        signature: &'b ir::Signature,
        ctx: &'b device::Context,
    ) -> Vec<Candidate<'b>>;

    /// Computes the expected output.
    fn get_expected_output(&self, &device::Context) -> Self::ExpectedOutput;

    /// Ensures the generated code performs the correct operation.
    fn check_result(
        &self,
        expected: &Self::ExpectedOutput,
        context: &device::Context,
    ) -> Result<(), String>;

    /// Generates, executes and tests the output of candidates for the kernel.
    fn test_correctness<AM>(params: Self::Parameters, num_tests: usize, context: &mut AM)
    where
        AM: device::ArgMap + device::Context + 'a,
    {
        let (sig, context) = KernelBuilder::default()
            .mem_init(MemInit::RandomFill)
            .build::<Self, AM>(params, context);
        let expected_output = sig.kernel().get_expected_output(context);
        let candidates = sig.build_candidates(context);
        let mut num_deadends = 0;
        let mut num_runs = 0;
        while num_runs < num_tests {
            let order = explorer::config::NewNodeOrder::WeightedRandom;
            let ordering = explorer::config::ChoiceOrdering::default();
            let bounds = candidates.iter().map(|c| c.bound.value()).enumerate();
            let candidate_idx = local_selection::pick_index(order, bounds, CUT);
            let candidate = candidates[unwrap!(candidate_idx)].clone();
            let leaf =
                local_selection::descend(&ordering, order, context, candidate, CUT);
            if let Some(leaf) = leaf {
                let device_fn = codegen::Function::build(&leaf.space);
                unwrap!(
                    context.evaluate(&device_fn, device::EvalMode::FindBest),
                    "evaluation failed for kernel {}, with actions {:?}",
                    Self::name(),
                    leaf.actions
                );
                if let Err(err) = sig.kernel().check_result(&expected_output, context) {
                    panic!(
                        "incorrect output for kernel {}, with actions {:?}: {}",
                        Self::name(),
                        leaf.actions,
                        err
                    )
                }
                num_runs += 1;
            } else {
                num_deadends += 1;
                if num_deadends as f32 / ((1 + num_deadends + num_runs) as f32)
                    >= MAX_DEADEND_RATIO
                {
                    panic!(
                        concat!(
                            "too many dead-ends for kernel {}, ",
                            "{} deadends for {} successful runs"
                        ),
                        Self::name(),
                        num_deadends,
                        num_runs
                    )
                }
            }
        }
    }

    /// Tests the correctness of the bound of kernels and returns the list of tested leafs
    /// along with the actual evaluation time.
    fn test_bound<AM>(
        params: Self::Parameters,
        num_tests: usize,
        mem_init: MemInit,
        context: &mut AM,
    ) -> Vec<BoundSample>
    where
        AM: device::ArgMap + device::Context + 'a,
    {
        let (sig, context) = KernelBuilder::default()
            .mem_init(mem_init)
            .build::<Self, AM>(params, context);
        let candidates = sig.build_candidates(context);
        let leaves = Mutex::new(Vec::new());
        let num_tested = atomic::AtomicUsize::new(0);
        context.async_eval(
            num_cpus::get(),
            device::EvalMode::TestBound,
            &|evaluator| loop {
                if num_tested.fetch_add(1, atomic::Ordering::SeqCst) >= num_tests {
                    if num_tested.fetch_sub(1, atomic::Ordering::SeqCst) > num_tests {
                        break;
                    }
                }
                if let Some((leaf, bounds)) = descend_check_bounds(&candidates, context) {
                    let leaves = &leaves;
                    evaluator.add_kernel(
                        leaf,
                        (move |leaf: Candidate, runtime: f64| {
                            let bound = leaf.bound.clone();
                            let mut leaves = unwrap!(leaves.lock());
                            let mut actions = leaf.actions.iter().cloned().collect_vec();
                            actions.reverse();
                            for (idx, partial_bound) in bounds.iter().enumerate() {
                                assert!(
                                    partial_bound.value() <= bound.value() * 1.01,
                                    "invalid inner bound: {} < {}, kernel {}, \
                                     actions {:?} then {:?}",
                                    partial_bound,
                                    bound,
                                    Self::name(),
                                    &actions[..idx],
                                    &actions[idx..]
                                );
                            }
                            info!("new evaluation: {:.2e}ns, bound {}", runtime, bound);
                            leaves.push(BoundSample {
                                actions,
                                bound,
                                runtime,
                            });
                        }).into(),
                    );
                } else {
                    num_tested.fetch_sub(1, atomic::Ordering::SeqCst);
                }
            },
        );
        unwrap!(leaves.into_inner())
    }

    /// Runs the search and benchmarks the resulting candidate.
    fn benchmark<AM>(
        config: &explorer::Config,
        params: Self::Parameters,
        num_samples: usize,
        mem_init: MemInit,
        context: &mut AM,
    ) -> Vec<f64>
    where
        AM: device::ArgMap + device::Context + 'a,
    {
        let (sig, context) = KernelBuilder::default()
            .mem_init(mem_init)
            .build::<Self, AM>(params, context);
        sig.benchmark(context, config, num_samples)
    }

    /// Computes the probability of encountering a dead-end when descending in the search
    /// tree.
    fn deadend_ratio<AM>(
        params: Self::Parameters,
        num_samples: usize,
        context: &mut AM,
    ) -> f64
    where
        AM: device::ArgMap + device::Context + 'a,
    {
        let (sig, context) = KernelBuilder::default()
            .mem_init(MemInit::Uninit)
            .build::<Self, AM>(params, context);
        let candidates = sig.build_candidates(context);
        let num_deadends = (0..num_samples)
            .into_par_iter()
            .filter(|_| {
                let order = explorer::config::NewNodeOrder::WeightedRandom;
                let ordering = explorer::config::ChoiceOrdering::default();
                let inf = std::f64::INFINITY;
                let bounds = candidates.iter().map(|c| c.bound.value()).enumerate();
                let candidate_idx = local_selection::pick_index(order, bounds, inf);
                let candidate = candidates[unwrap!(candidate_idx)].clone();
                local_selection::descend(&ordering, order, context, candidate, inf)
                    .is_none()
            }).count();
        num_deadends as f64 / num_samples as f64
    }
}

/// Memory initialization strategies.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum MemInit {
    /// Memory is left uninitialized
    Uninit,
    /// Memory is randomly filled with zeroes
    RandomFill,
}

#[derive(Debug, Clone)]
pub struct KernelBuilder<'a> {
    name: Option<Cow<'a, str>>,
    mem_init: MemInit,
}

impl<'a> Default for KernelBuilder<'a> {
    fn default() -> Self {
        KernelBuilder {
            // Defaults to kernel name
            name: None,
            mem_init: MemInit::Uninit,
        }
    }
}

impl<'a> KernelBuilder<'a> {
    /// Sets the name of the generated kernel.  This will appear in log files.
    pub fn name<T: Into<Cow<'static, str>>>(mut self, name: T) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the memory initialization strategy.  See `MemInit` for details.
    pub fn mem_init(mut self, mem_init: MemInit) -> Self {
        self.mem_init = mem_init;
        self
    }

    fn get_kernel_name<K: Kernel<'a>>(&self) -> Cow<'a, str> {
        self.name
            .as_ref()
            .map(Cow::to_owned)
            .unwrap_or_else(|| K::name().into())
    }

    /// Create a kernel in the given context.  This returns a frozen reference to the context, the
    /// kernel, and its signature.
    pub fn build<'b, K, AM>(
        &self,
        params: K::Parameters,
        context: &'b mut AM,
    ) -> (SignedKernel<'a, K>, &'b AM)
    where
        AM: device::ArgMap + device::Context + 'a,
        K: Kernel<'a> + 'b,
    {
        let name = self.get_kernel_name::<K>();
        let (kernel, signature);
        {
            let mut builder = SignatureBuilder::new(&name, context);
            builder.set_random_fill(match self.mem_init {
                MemInit::RandomFill => true,
                MemInit::Uninit => false,
            });
            kernel = K::build_signature(params, &mut builder);
            signature = builder.get();
        }

        (
            SignedKernel {
                name,
                signature,
                kernel,
            },
            context,
        )
    }
}

/// A kernel together with its IR signature.
pub struct SignedKernel<'a, K: ?Sized> {
    name: Cow<'a, str>,
    signature: ir::Signature,
    kernel: K,
}

impl<'a, K: ?Sized> SignedKernel<'a, K>
where
    K: ErasedKernel + 'a,
{
    pub fn signature(&self) -> &ir::Signature {
        &self.signature
    }

    pub fn kernel(&self) -> &K {
        &self.kernel
    }

    pub fn build_candidates<'b>(
        &'b self,
        context: &'b dyn device::Context,
    ) -> Vec<Candidate<'b>> {
        self.kernel().build_body(self.signature(), context)
    }

    pub fn benchmark(
        &self,
        context: &dyn device::Context,
        config: &explorer::Config,
        num_samples: usize,
    ) -> Vec<f64> {
        let candidates = self.build_candidates(context);
        let best = unwrap!(
            explorer::find_best_ex(config, context, candidates),
            "no candidates found for kernel {}",
            self.name,
        );
        let best_fn = codegen::Function::build(&best.space);
        context.benchmark(&best_fn, num_samples)
    }
}

/// A type alias for type-erased kernels
pub type DynKernel<'a> = SignedKernel<'a, dyn ErasedKernel + 'a>;

/// An object-safe trait independent of the kernel to create type-erased candidates.
pub trait ErasedKernel {
    fn build_body<'b>(
        &self,
        signature: &'b ir::Signature,
        context: &'b dyn device::Context,
    ) -> Vec<Candidate<'b>>;
}

impl<'a, K> ErasedKernel for K
where
    K: Kernel<'a>,
{
    fn build_body<'b>(
        &self,
        signature: &'b ir::Signature,
        context: &'b dyn device::Context,
    ) -> Vec<Candidate<'b>> {
        self.build_body(signature, context)
    }
}

/// Descend along a path in the search tree and stores the bounds encountered on the way.
fn descend_check_bounds<'a>(
    candidates: &[Candidate<'a>],
    context: &device::Context,
) -> Option<(Candidate<'a>, Vec<Bound>)> {
    let order = explorer::config::NewNodeOrder::WeightedRandom;
    let mut candidates = std::borrow::Cow::Borrowed(candidates);
    let mut bounds = Vec::new();
    loop {
        let idx = if let Some(idx) = {
            let idx_bounds = candidates.iter().map(|c| c.bound.value()).enumerate();
            local_selection::pick_index(order, idx_bounds, CUT)
        } {
            idx
        } else {
            return None;
        };
        bounds.push(candidates[idx].bound.clone());
        let choice_opt = explorer::choice::default_list(&candidates[idx].space).next();
        if let Some(choice) = choice_opt {
            let new_nodes = candidates[idx]
                .apply_choice(context, choice)
                .into_iter()
                .filter(|x| x.bound.value() < CUT)
                .collect_vec();
            candidates = std::borrow::Cow::Owned(new_nodes);
        } else {
            return Some((candidates[idx].clone(), bounds));
        }
    }
}

/// A sample of the accuracy of bounds.
pub struct BoundSample {
    actions: Vec<explorer::choice::ActionEx>,
    bound: Bound,
    runtime: f64,
}

impl BoundSample {
    /// Returns the ratio between the bound and the actual evaluation.
    fn ratio(&self) -> f64 {
        self.runtime / self.bound.value()
    }
}

impl std::fmt::Display for BoundSample {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{:.2}x, {:.2e}ns vs {}, for actions {:?}",
            self.ratio(),
            self.runtime,
            self.bound,
            self.actions
        )
    }
}

/// Prints an analysis of the bounds computed by the lower bound model.
pub fn analyze_bounds(mut bounds: Vec<BoundSample>) {
    const NUM_QUANTILES: usize = 5;
    bounds.sort_by(|x, y| cmp_f64(x.ratio(), y.ratio()));
    let num_errors = bounds.iter().take_while(|b| b.ratio() < 1.).count();
    if num_errors > 0 {
        let error_ratio = num_errors as f64 / bounds.len() as f64;
        let error_ratio = statistics::estimate_ratio(error_ratio, bounds.len());
        println!("ratio of errors {}, for example: ", error_ratio);
        let num_printed = std::cmp::min(NUM_QUANTILES, num_errors);
        for i in 0..num_printed {
            let index = i * num_errors / num_printed;
            println!("{}% worst error: {}", i * 100 / num_printed, bounds[index]);
        }
    }
    if num_errors < bounds.len() {
        let num_bounds = bounds.len() - num_errors;
        let num_quantiles = std::cmp::min(NUM_QUANTILES, num_bounds);
        for i in 0..num_quantiles {
            let index = (i + 1) * (num_bounds / num_quantiles) - 1;
            println!(
                "{}% worst: {}",
                (i + 1) * 100 / num_quantiles,
                bounds[num_errors + index]
            );
        }
    }
}
