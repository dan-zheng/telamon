use log::{info, trace};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter;
use std::ops::{self, Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};
use std::rc::Rc;

use itertools::Itertools;
use num::{BigUint, Integer, One, ToPrimitive, Zero};

mod hash;

fn is_close(lhs: f64, rhs: f64) -> bool {
    (lhs - rhs).abs() < 1e-8 + 1e-5 * rhs.abs()
}

fn lt_close(lhs: f64, rhs: f64) -> bool {
    lhs < rhs || is_close(lhs, rhs)
}

fn gt_close(lhs: f64, rhs: f64) -> bool {
    lhs > rhs || is_close(lhs, rhs)
}

pub trait Range<N = u64> {
    fn min_value(&self) -> N;

    fn max_value(&self) -> N;
}

pub trait Factor<N = u64> {
    fn gcd_value(&self) -> N;

    fn lcm_value(&self) -> N;
}

pub trait Atom:
    Range + Factor + Clone + fmt::Debug + fmt::Display + PartialEq + Ord + Hash
{
}

impl<T> Atom for T where
    T: ?Sized
        + Range
        + Factor
        + Clone
        + fmt::Debug
        + fmt::Display
        + PartialEq
        + Ord
        + Hash
{
}

// implements "T op U" based on top of "T op &U"
macro_rules! forward_val_val_binop {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<$($gen)*> $imp<$u> for $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: $u) -> $t {
                $imp::$method(self, &other)
            }
        }
    };
}

// implements "T op U" based on top of "U op T" where op is commutative
macro_rules! forward_val_val_binop_commutative {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<$($gen)*> $imp<$u> for $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: $u) -> $o {
                $imp::$method(other, self)
            }
        }
    };
}

// implements "&T op U" based on top of "&T op &U"
macro_rules! forward_ref_val_binop {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<'a, $($gen)*> $imp<$u> for &'a $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: $u) -> $o {
                $imp::$method(self, &other)
            }
        }
    };
}

// implements "&T op U" based on top of "U op &T" where op is commutative
macro_rules! forward_ref_val_binop_commutative {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<'a, $($gen)*> $imp<$u> for &'a $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: $u) -> $o {
                $imp::$method(other, self)
            }
        }
    };
}

// implements "T op &U" based on top of "&T op &U"
macro_rules! forward_val_ref_binop {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<'a, $($gen)*> $imp<&'a $u> for $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: &'a $u) -> $o {
                $imp::$method(&self, other)
            }
        }
    };
}

// implements "&T op &U" based on top of "T op &U" where T is expected to be `Clone`able
macro_rules! forward_ref_ref_binop {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<'a, 'b, $($gen)*> $imp<&'a $u> for &'b $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: &'a $u) -> $o {
                $imp::$method(self.clone(), other)
            }
        }
    };
}

// implements "&T op &U" based on top of "&U op &T" where op is commutative
macro_rules! forward_ref_ref_binop_commutative {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        impl<'a, 'b, $($gen)*> $imp<&'a $u> for &'b $t {
            type Output = $o;

            #[inline]
            fn $method(self, other: &'a $u) -> $o {
                $imp::$method(other, self)
            }
        }
    };
}

// forward all to "&T op &U"
macro_rules! forward_binop_to_ref_ref {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        forward_val_val_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_val_ref_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_val_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
    };
}

// forward all to "T op &U" where T is expected to be `Clone`able
macro_rules! forward_binop_to_val_ref {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        forward_val_val_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_val_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_ref_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
    };
}

// forward "T op U", "&T op U" and "&T op &U" to "T op &U" or "U op &T" where T is expected to be
// `Clone`able and op is commutative
macro_rules! forward_binop_to_val_ref_commutative {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        forward_val_val_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_val_binop_commutative!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_ref_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
    };
}

// implements "T op= U" based on top of "T op= &U"
macro_rules! forward_val_op_assign {
    (impl($($gen:tt)*) $imp:ident, $method:ident for $t:ty, $u:ty) => {
        impl<$($gen)*> $imp<$u> for $t {
            #[inline]
            fn $method(&mut self, other: $u) {
                $imp::$method(self, &other)
            }
        }
    };
}

// implements "T op &U" based on top of "T op= &U"
macro_rules! forward_val_ref_to_op_assign {
    (impl($($gen:tt)*) $imp:ident, $method:ident for $t:ty, $u:ty, $imp_assign:ident, $method_assign:ident) => {
        impl<'a, $($gen)*> $imp<&'a $u> for $t {
            type Output = $t;

            #[inline]
            fn $method(mut self, other: &'a $u) -> $t {
                $imp_assign::$method_assign(&mut self, other);
                self
            }
        }
    };
}

// forward "T op U", "T op &U", "&T op U", "&T op &U" and "T op= U" to "T op= &U" where T is
// expected to be `Clone`able
macro_rules! forward_binop_to_op_assign {
    (impl($($gen:tt)*) $imp:ident, $method:ident for $t:ty, $u:ty, $imp_assign:ident, $method_assign:ident) => {
        forward_binop_to_val_ref!(impl($($gen)*) $imp<Output = $t>, $method for $t, $u);
        forward_val_ref_to_op_assign!(impl($($gen)*) $imp, $method for $t, $u, $imp_assign, $method_assign);
        forward_val_op_assign!(impl($($gen)*) $imp_assign, $method_assign for $t, $u);
    };
}

macro_rules! forward_binop {
    (impl($($gen:tt)*) $imp:ident, $method:ident for $t:ty, $u:ty, $imp_assign:ident, $method_assign:ident) => {
        forward_val_val_binop!(impl($($gen)*) $imp<Output = $t>, $method for $t, $u);
        forward_ref_val_binop!(impl($($gen)*) $imp<Output = $t>, $method for $t, $u);
        forward_val_ref_to_op_assign!(impl($($gen)*) $imp, $method for $t, $u, $imp_assign, $method_assign);
        forward_val_op_assign!(impl($($gen)*) $imp_assign, $method_assign for $t, $u);
    };
}

macro_rules! forward_binop_commutative {
    (impl($($gen:tt)*) $imp:ident, $method:ident for $t:ty, $u:ty, $imp_assign:ident, $method_assign:ident) => {
        forward_val_val_binop!(impl($($gen)*) $imp<Output = $t>, $method for $t, $u);
        forward_ref_val_binop_commutative!(impl($($gen)*) $imp<Output = $t>, $method for $t, $u);
        forward_val_ref_to_op_assign!(impl($($gen)*) $imp, $method for $t, $u, $imp_assign, $method_assign);
        forward_val_op_assign!(impl($($gen)*) $imp_assign, $method_assign for $t, $u);
    };
}

// forward "T op U", "T op &U", "&T op U", "&T op &U" and "T op= U" to "T op= &U" or "U op &T"
// where T is expected to be `Clone`able and op is commutative
macro_rules! forward_binop_to_op_assign_commutative {
    (impl($($gen:tt)*) $imp:ident, $method:ident for $t:ty, $u:ty, $imp_assign:ident, $method_assign:ident) => {
        forward_binop_to_val_ref_commutative!(impl($($gen)*) $imp<Output = $t>, $method for $t, $u);
        forward_val_ref_to_op_assign!(impl($($gen)*) $imp, $method for $t, $u, $imp_assign, $method_assign);
        forward_val_op_assign!(impl($($gen)*) $imp_assign, $method_assign for $t, $u);
    };
}

// forward "T op U", "T op &U", "&T op &U" and "&T op U" to "U op &T" where op is commutative
macro_rules! forward_binop_to_ref_val_commutative {
    (impl($($gen:tt)*) $imp:ident<Output = $o:ty>, $method:ident for $t:ty, $u:ty) => {
        forward_val_val_binop_commutative!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_val_ref_binop!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_ref_binop_commutative!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
        forward_ref_val_binop_commutative!(impl($($gen)*) $imp<Output = $o>, $method for $t, $u);
    };
}

// Integer division of parameters.  There is the assumption that whatever values of the
// parameters are chosen, the ratio is an integer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RatioInner<P> {
    factor: BigUint,
    numer: Vec<P>,
    denom: Vec<P>,
}

impl<P> fmt::Display for RatioInner<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.numer.is_empty() {
            write!(fmt, "{}", self.factor)?;
        } else {
            if !self.factor.is_one() {
                write!(fmt, "{}*", self.factor)?;
            }

            write!(fmt, "{}", self.numer.iter().format("*"))?;
        }

        if !self.denom.is_empty() {
            write!(fmt, "/{}", self.denom.iter().format("/"))?;
        }

        Ok(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ratio<P> {
    inner: Rc<RatioInner<P>>,
}

impl<P> fmt::Debug for Ratio<P>
where
    P: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Ratio")
            .field("factor", &self.inner.factor)
            .field("numer", &self.inner.numer)
            .field("denom", &self.inner.denom)
            .finish()
    }
}

impl<P> fmt::Display for Ratio<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.inner, fmt)
    }
}

impl<P> Clone for Ratio<P> {
    fn clone(&self) -> Self {
        Ratio {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<P> From<RatioInner<P>> for Ratio<P>
where
    P: Atom,
{
    fn from(inner: RatioInner<P>) -> Self {
        Ratio {
            inner: Rc::new(inner),
        }
    }
}

impl<'a, P: 'a> iter::Product<&'a Ratio<P>> for Ratio<P>
where
    P: Atom,
{
    fn product<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a Ratio<P>>,
    {
        let mut res = Self::one();
        for elem in iter {
            res *= elem;
        }
        res
    }
}

impl<P> Ratio<P>
where
    P: Atom,
{
    pub fn new(factor: BigUint, mut numer: Vec<P>, mut denom: Vec<P>) -> Self {
        numer.sort();
        denom.sort();

        RatioInner {
            factor,
            numer,
            denom,
        }
        .into()
    }

    pub fn one() -> Self {
        Self::new(1u32.into(), Vec::new(), Vec::new())
    }

    pub fn to_symbolic_float(&self) -> Float<P> {
        FloatInner::Mul(self.clone().into(), Vec::new()).into()
    }

    fn to_u32(&self) -> Option<u32> {
        if self.inner.numer.is_empty() && self.inner.denom.is_empty() {
            Some(
                self.inner
                    .factor
                    .to_u32()
                    .unwrap_or_else(|| panic!("Unable to represent factor as u32")),
            )
        } else {
            None
        }
    }

    fn as_biguint(&self) -> Option<&BigUint> {
        if self.inner.numer.is_empty() && self.inner.denom.is_empty() {
            Some(&self.inner.factor)
        } else {
            None
        }
    }

    fn is_multiple_of(&self, other: &Ratio<P>) -> bool {
        use itertools::EitherOrBoth::*;

        let (left_gcd, right_lcm) =
            (self.inner.factor.clone(), other.inner.factor.clone());
        let (left_gcd, right_lcm) = self
            .inner
            .numer
            .iter()
            .merge_join_by(&other.inner.numer, |lhs, rhs| lhs.cmp(rhs))
            .fold(
                (left_gcd, right_lcm),
                |(left_gcd, right_lcm), either| match either {
                    Left(lhs) => (left_gcd * lhs.gcd_value(), right_lcm),
                    Right(rhs) => (left_gcd, right_lcm * rhs.lcm_value()),
                    Both(_, _) => (left_gcd, right_lcm),
                },
            );
        let (left_gcd, right_lcm) = self
            .inner
            .denom
            .iter()
            .merge_join_by(&other.inner.denom, |lhs, rhs| lhs.cmp(rhs))
            .fold(
                (left_gcd, right_lcm),
                |(left_gcd, right_lcm), either| match either {
                    Left(lhs) => (left_gcd / lhs.lcm_value(), right_lcm),
                    Right(rhs) => (left_gcd, right_lcm / rhs.gcd_value()),
                    Both(_, _) => (left_gcd, right_lcm),
                },
            );
        left_gcd.is_multiple_of(&right_lcm)
    }

    fn is_greater_than(&self, other: &Ratio<P>) -> bool {
        use itertools::EitherOrBoth::*;

        let (left_min, right_max) =
            (self.inner.factor.clone(), other.inner.factor.clone());
        let (left_min, right_max) = self
            .inner
            .numer
            .iter()
            .merge_join_by(&other.inner.numer, |lhs, rhs| lhs.cmp(rhs))
            .fold(
                (left_min, right_max),
                |(left_min, right_max), either| match either {
                    Left(lhs) => (left_min * lhs.min_value(), right_max),
                    Right(rhs) => (left_min, right_max * rhs.max_value()),
                    Both(_, _) => (left_min, right_max),
                },
            );
        let (left_min, right_max) = self
            .inner
            .denom
            .iter()
            .merge_join_by(&other.inner.denom, |lhs, rhs| lhs.cmp(rhs))
            .fold(
                (left_min, right_max),
                |(left_min, right_max), either| match either {
                    Left(lhs) => (left_min, right_max * lhs.max_value()),
                    Right(rhs) => (left_min * rhs.min_value(), right_max),
                    Both(_, _) => (left_min, right_max),
                },
            );

        left_min >= right_max
    }

    fn is_less_than(&self, other: &Ratio<P>) -> bool {
        other.is_greater_than(self)
    }
}

impl<P> Range for RatioInner<P>
where
    P: Range,
{
    fn min_value(&self) -> u64 {
        let factor = self
            .factor
            .to_u64()
            .unwrap_or_else(|| panic!("Unable to represent factor as u64"));
        let numer_min = self.numer.iter().map(Range::min_value).product::<u64>();
        let denom_max = self.denom.iter().map(Range::max_value).product::<u64>();
        (factor * numer_min) / denom_max
    }

    fn max_value(&self) -> u64 {
        let factor = self
            .factor
            .to_u64()
            .unwrap_or_else(|| panic!("Unable to represent factor as u64"));
        let numer_max = self.numer.iter().map(Range::max_value).product::<u64>();
        let denom_min = self.denom.iter().map(Range::min_value).product::<u64>();
        (factor * numer_max) / denom_min
    }
}

impl<P> Factor for RatioInner<P>
where
    P: Factor,
{
    fn gcd_value(&self) -> u64 {
        let factor = self
            .factor
            .to_u64()
            .unwrap_or_else(|| panic!("Unable to represent factor as u64"));
        let numer_gcd = self.numer.iter().map(Factor::gcd_value).product::<u64>();
        let denom_lcm = self.denom.iter().map(Factor::lcm_value).product::<u64>();
        assert!((factor * numer_gcd).is_multiple_of(&denom_lcm));

        (factor * numer_gcd) / denom_lcm
    }

    fn lcm_value(&self) -> u64 {
        let factor = self
            .factor
            .to_u64()
            .unwrap_or_else(|| panic!("Unable to represent factor as u64"));
        let numer_lcm = self.numer.iter().map(Factor::lcm_value).product::<u64>();
        let denom_gcd = self.denom.iter().map(Factor::gcd_value).product::<u64>();
        assert!((factor * numer_lcm).is_multiple_of(&denom_gcd));

        (factor * numer_lcm) / denom_gcd
    }
}

impl<P> Range for Ratio<P>
where
    P: Range,
{
    fn min_value(&self) -> u64 {
        self.inner.min_value()
    }

    fn max_value(&self) -> u64 {
        self.inner.max_value()
    }
}

impl<P> Factor for Ratio<P>
where
    P: Factor,
{
    fn gcd_value(&self) -> u64 {
        self.inner.gcd_value()
    }

    fn lcm_value(&self) -> u64 {
        self.inner.lcm_value()
    }
}

impl<'a, 'b, P> ops::Mul<&'a BigUint> for &'b Ratio<P>
where
    P: Atom,
{
    type Output = Ratio<P>;

    fn mul(self, other: &'a BigUint) -> Ratio<P> {
        Ratio::new(
            &self.inner.factor * other,
            self.inner.numer.clone(),
            self.inner.denom.clone(),
        )
    }
}

impl<P> ops::MulAssign<&'_ BigUint> for Ratio<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ BigUint) {
        Rc::make_mut(&mut self.inner).factor *= other;
    }
}

forward_binop!(impl(P: Atom) Mul, mul for Ratio<P>, BigUint, MulAssign, mul_assign);

impl<'a, 'b, P> ops::Mul<&'a Ratio<P>> for &'b Ratio<P>
where
    P: Atom,
{
    type Output = Ratio<P>;

    fn mul(self, other: &'a Ratio<P>) -> Ratio<P> {
        use itertools::EitherOrBoth::*;

        let mut numer =
            Vec::with_capacity(self.inner.numer.len() + other.inner.numer.len());
        let mut denom =
            Vec::with_capacity(other.inner.denom.len() + self.inner.denom.len());
        for either in self
            .inner
            .numer
            .iter()
            .merge_join_by(other.inner.denom.iter(), |lhs, rhs| lhs.cmp(rhs))
        {
            match either {
                Left(lhs) => numer.push(lhs.clone()),
                Right(rhs) => denom.push(rhs.clone()),
                Both(_, _) => (),
            }
        }

        for either in self
            .inner
            .denom
            .iter()
            .merge_join_by(other.inner.numer.iter(), |lhs, rhs| lhs.cmp(rhs))
        {
            match either {
                Left(lhs) => denom.push(lhs.clone()),
                Right(rhs) => numer.push(rhs.clone()),
                Both(_, _) => (),
            }
        }

        // Ratio::new sorts its arguments
        Ratio::new(&self.inner.factor * &other.inner.factor, numer, denom)
    }
}

impl<P> ops::MulAssign<&'_ Ratio<P>> for Ratio<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, rhs: &'_ Ratio<P>) {
        let lhs = Rc::make_mut(&mut self.inner);
        lhs.factor *= &rhs.inner.factor;
        for numer in rhs.inner.numer.iter() {
            if let Some(pos) = lhs.denom.iter().position(|d| d == numer) {
                lhs.denom.swap_remove(pos);
            } else {
                lhs.numer.push(numer.clone());
            }
        }
        for denom in rhs.inner.denom.iter() {
            if let Some(pos) = lhs.numer.iter().position(|n| n == denom) {
                lhs.numer.swap_remove(pos);
            } else {
                lhs.denom.push(denom.clone());
            }
        }

        lhs.numer.sort();
        lhs.denom.sort();
    }
}

forward_binop_commutative!(impl(P: Atom) Mul, mul for Ratio<P>, Ratio<P>, MulAssign, mul_assign);

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct LcmExpr<P>
where
    P: Atom,
{
    gcd: BigUint,
    lcm: BigUint,
    args: VecSet<Ratio<P>>,
}

impl<P> fmt::Display for LcmExpr<P>
where
    P: Atom,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.args.is_empty() {
            write!(fmt, "lcm({})", self.lcm)
        } else {
            write!(fmt, "lcm({}, {})", self.gcd, self.args.iter().format(", "))
        }
    }
}

impl<P> From<LcmExpr<P>> for Int<P>
where
    P: Atom,
{
    fn from(lmin: LcmExpr<P>) -> Self {
        lmin.simplified()
            .map(|ratio| IntInner::Mul(ratio).into())
            .unwrap_or_else(|lcm| IntInner::Lcm(lcm).into())
    }
}

impl<P> From<Ratio<P>> for LcmExpr<P>
where
    P: Atom,
{
    fn from(value: Ratio<P>) -> Self {
        LcmExpr {
            gcd: value.gcd_value().into(),
            lcm: value.lcm_value().into(),
            args: VecSet::from_sorted_iter(iter::once(value)),
        }
    }
}

impl<'a, 'b, P> ops::Mul<&'a BigUint> for &'b LcmExpr<P>
where
    P: Atom,
{
    type Output = LcmExpr<P>;

    fn mul(self, other: &'a BigUint) -> LcmExpr<P> {
        LcmExpr {
            gcd: &self.gcd * other,
            lcm: &self.lcm * other,
            args: self.args.iter().map(|arg| arg * other).collect(),
        }
    }
}

impl<P> ops::MulAssign<&'_ BigUint> for LcmExpr<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ BigUint) {
        self.gcd *= other;
        self.lcm *= other;
        self.args.iter_mut().for_each(|arg| *arg *= other);
    }
}

forward_binop!(impl(P: Atom) Mul, mul for LcmExpr<P>, BigUint, MulAssign, mul_assign);

impl<'a, 'b, P> ops::Mul<&'a Ratio<P>> for &'b LcmExpr<P>
where
    P: Atom,
{
    type Output = LcmExpr<P>;

    fn mul(self, other: &'a Ratio<P>) -> LcmExpr<P> {
        if let Some(val) = other.as_biguint() {
            self * val
        } else {
            LcmExpr {
                gcd: &self.gcd * other.gcd_value(),
                lcm: &self.lcm * other.lcm_value(),
                args: self.args.iter().map(|arg| arg * other).collect(),
            }
        }
    }
}

impl<P> ops::MulAssign<&'_ Ratio<P>> for LcmExpr<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ Ratio<P>) {
        if let Some(val) = other.as_biguint() {
            *self *= val;
        } else {
            self.gcd *= other.gcd_value();
            self.lcm *= other.lcm_value();
            self.args.iter_mut().for_each(|arg| *arg *= other);
        }
    }
}

forward_binop!(impl(P: Atom) Mul, mul for LcmExpr<P>, Ratio<P>, MulAssign, mul_assign);

impl<P> LcmExpr<P>
where
    P: Atom,
{
    pub fn new<II>(iter: II) -> Option<Self>
    where
        II: IntoIterator<Item = Ratio<P>>,
    {
        let mut iter = iter.into_iter();

        if let Some(first) = iter.next() {
            let mut gcd = BigUint::from(first.gcd_value());
            let mut lcm = BigUint::from(first.lcm_value());

            let mut args: Vec<Ratio<P>> =
                Vec::with_capacity(iter.size_hint().1.unwrap_or(0));
            args.push(first);

            'elem: for elem in iter {
                let mut to_remove = vec![];
                for (ix, arg) in args.iter().enumerate() {
                    if arg.is_multiple_of(&elem) {
                        continue 'elem;
                    }

                    if elem.is_multiple_of(&arg) {
                        to_remove.push(ix);
                    }
                }

                // Need to iterate backwards so that indices stay valid
                for ix in to_remove.into_iter().rev() {
                    args.remove(ix);
                }

                gcd = gcd.lcm(&BigUint::from(elem.gcd_value()));
                lcm = lcm.lcm(&BigUint::from(elem.lcm_value()));
                args.push(elem);
            }

            Some(LcmExpr {
                gcd,
                lcm,
                args: args.into_iter().collect(),
            })
        } else {
            None
        }
    }

    pub fn one() -> Self {
        LcmExpr {
            gcd: 1u32.into(),
            lcm: 1u32.into(),
            args: VecSet::default(),
        }
    }

    fn as_biguint(&self) -> Option<&BigUint> {
        if self.is_constant() {
            Some(&self.lcm)
        } else {
            None
        }
    }

    fn to_u32(&self) -> Option<u32> {
        self.as_biguint().map(|value| {
            value
                .to_u32()
                .expect("Unable to convert constant lcm to u32")
        })
    }

    fn is_constant(&self) -> bool {
        self.gcd == self.lcm
    }

    fn is_single_value(&self) -> bool {
        self.args.len() == 1 && self.gcd == BigUint::from(self.args[0].gcd_value())
    }

    fn simplified(self) -> Result<Ratio<P>, Self> {
        if self.is_constant() {
            Ok(Ratio::new(self.gcd, Vec::new(), Vec::new()))
        } else if self.is_single_value() {
            Ok(self.args.values[0].clone())
        } else {
            Err(self)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct MinExpr<P>
where
    P: Atom,
{
    min: BigUint,
    max: BigUint,
    values: VecSet<Ratio<P>>,
}

impl<P> fmt::Display for MinExpr<P>
where
    P: Atom,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if let Some(minmax) = self.values.iter().map(Range::max_value).min() {
            if BigUint::from(minmax) == self.max {
                write!(fmt, "min({})", self.values.iter().format(", "))
            } else {
                write!(
                    fmt,
                    "min({}, {})",
                    self.max,
                    self.values.iter().format(", ")
                )
            }
        } else {
            assert!(self.values.is_empty());
            assert_eq!(self.min, self.max);

            write!(fmt, "min({})", self.min)
        }
    }
}

impl<P> From<MinExpr<P>> for Int<P>
where
    P: Atom,
{
    fn from(emin: MinExpr<P>) -> Self {
        emin.simplified()
            .map(|ratio| IntInner::Mul(ratio).into())
            .unwrap_or_else(|min| IntInner::Min(min).into())
    }
}

impl<P> From<Ratio<P>> for Int<P>
where
    P: Atom,
{
    fn from(ratio: Ratio<P>) -> Self {
        IntInner::Mul(ratio).into()
    }
}

impl<P> From<Ratio<P>> for MinExpr<P>
where
    P: Atom,
{
    fn from(value: Ratio<P>) -> Self {
        MinExpr {
            min: value.min_value().into(),
            max: value.max_value().into(),
            values: VecSet::from_sorted_iter(iter::once(value)),
        }
    }
}

impl<P> From<u32> for MinExpr<P>
where
    P: Atom,
{
    fn from(constant: u32) -> Self {
        MinExpr {
            min: constant.into(),
            max: constant.into(),
            values: VecSet::default(),
        }
    }
}

impl<'a, 'b, P> ops::Mul<&'a BigUint> for &'b MinExpr<P>
where
    P: Atom,
{
    type Output = MinExpr<P>;

    fn mul(self, other: &'a BigUint) -> MinExpr<P> {
        MinExpr {
            min: &self.min * other,
            max: &self.max * other,
            values: self.values.iter().map(|arg| arg * other).collect(),
        }
    }
}

impl<P> ops::MulAssign<&'_ BigUint> for MinExpr<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ BigUint) {
        self.min *= other;
        self.max *= other;
        self.values.iter_mut().for_each(|arg| *arg *= other);
    }
}

forward_binop!(impl(P: Atom) Mul, mul for MinExpr<P>, BigUint, MulAssign, mul_assign);

impl<'a, 'b, P> ops::Mul<&'a Ratio<P>> for &'b MinExpr<P>
where
    P: Atom,
{
    type Output = MinExpr<P>;

    fn mul(self, other: &'a Ratio<P>) -> MinExpr<P> {
        if let Some(val) = other.as_biguint() {
            self * val
        } else {
            MinExpr {
                min: &self.min * other.min_value(),
                max: &self.max * other.max_value(),
                values: self.values.iter().map(|arg| arg * other).collect(),
            }
        }
    }
}

impl<P> ops::MulAssign<&'_ Ratio<P>> for MinExpr<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ Ratio<P>) {
        if let Some(val) = other.as_biguint() {
            *self *= val;
        } else {
            self.min *= other.min_value();
            self.max *= other.max_value();
            self.values.iter_mut().for_each(|arg| *arg *= other);
        }
    }
}

forward_binop!(impl(P: Atom) Mul, mul for MinExpr<P>, Ratio<P>, MulAssign, mul_assign);

impl<P> MinExpr<P>
where
    P: Atom,
{
    pub fn new<II>(iter: II) -> Option<Self>
    where
        II: IntoIterator<Item = Ratio<P>>,
    {
        let mut iter = iter.into_iter();

        if let Some(first) = iter.next() {
            let mut min = BigUint::from(first.min_value());
            let mut max = BigUint::from(first.max_value());
            let mut args: Vec<Ratio<P>> =
                Vec::with_capacity(iter.size_hint().1.unwrap_or(0));
            args.push(first);

            'elem: for elem in iter {
                if max <= BigUint::from(elem.min_value()) {
                    continue;
                }

                let mut to_remove = vec![];
                for (ix, arg) in args.iter().enumerate() {
                    if arg.is_less_than(&elem) {
                        continue 'elem;
                    }

                    if elem.is_less_than(&arg) {
                        to_remove.push(ix);
                    }
                }

                // Need to iterate backwards so that indices stay valid
                for ix in to_remove.into_iter().rev() {
                    args.remove(ix);
                }

                min = min.min(BigUint::from(elem.min_value()));
                max = max.min(BigUint::from(elem.max_value()));
                args.push(elem);
            }

            Some(MinExpr {
                min,
                max,
                values: args.into_iter().collect(),
            })
        } else {
            None
        }
    }

    pub fn one() -> Self {
        1u32.into()
    }

    pub fn to_symbolic_float(&self) -> Float<P> {
        self.clone()
            .simplified()
            .map(|ratio| ratio.to_symbolic_float())
            .unwrap_or_else(|emin| {
                FloatInner::Min(FMinExpr::new(
                    emin.min.to_u32().unwrap() as f64,
                    emin.max.to_u32().unwrap() as f64,
                    emin.values
                        .iter()
                        .cloned()
                        .map(FloatRatioInner::from)
                        .map(FloatRatio::from)
                        .map(DiffExpr::from)
                        .collect(),
                ))
                .into()
            })
    }

    fn as_biguint(&self) -> Option<&BigUint> {
        if self.is_constant() {
            debug_assert_eq!(self.min, self.max);

            Some(&self.min)
        } else {
            None
        }
    }

    fn to_u32(&self) -> Option<u32> {
        self.as_biguint().map(|value| value.to_u32().unwrap())
    }

    fn is_constant(&self) -> bool {
        self.min == self.max
    }

    fn is_single_value(&self) -> bool {
        self.values.len() == 1 && self.max == BigUint::from(self.values[0].max_value())
    }

    fn simplified(self) -> Result<Ratio<P>, Self> {
        if self.is_constant() {
            debug_assert_eq!(self.min, self.max);

            Ok(Ratio::new(self.min, Vec::new(), Vec::new()))
        } else if self.is_single_value() {
            debug_assert!(self.values.len() == 1);

            Ok(self.values.values[0].clone())
        } else {
            Err(self)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum IntInner<P>
where
    P: Atom,
{
    Lcm(LcmExpr<P>),
    Min(MinExpr<P>),
    Mul(Ratio<P>),
}

impl<P> fmt::Display for IntInner<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            IntInner::Lcm(lcm) => write!(fmt, "{}", lcm),
            IntInner::Min(min) => write!(fmt, "{}", min),
            IntInner::Mul(ratio) => write!(fmt, "{}", ratio),
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Int<P>
where
    P: Atom,
{
    inner: Rc<IntInner<P>>,
}

impl<P> fmt::Debug for Int<P>
where
    P: Atom + fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, fmt)
    }
}

impl<P> fmt::Display for Int<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.inner, fmt)
    }
}

impl<P> Clone for Int<P>
where
    P: Atom,
{
    fn clone(&self) -> Self {
        Int {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<P> From<IntInner<P>> for Int<P>
where
    P: Atom,
{
    fn from(inner: IntInner<P>) -> Self {
        Int {
            inner: Rc::new(inner),
        }
    }
}

impl<P, T> From<T> for Int<P>
where
    P: Atom,
    T: Into<BigUint>,
{
    fn from(inner: T) -> Self {
        Int::ratio(inner, Vec::new(), Vec::new())
    }
}

impl<P> Int<P>
where
    P: Atom,
{
    pub fn to_symbolic_float(&self) -> Float<P> {
        match &*self.inner {
            IntInner::Min(emin) => emin.to_symbolic_float(),
            IntInner::Mul(ratio) => ratio.to_symbolic_float(),
            IntInner::Lcm(_) => unimplemented!("Can't convert {} to float", self),
        }
    }

    pub fn ratio<T: Into<BigUint>>(factor: T, numer: Vec<P>, denom: Vec<P>) -> Self {
        IntInner::Mul(Ratio::new(factor.into(), numer, denom)).into()
    }

    pub fn one() -> Self {
        Self::from(1u32)
    }

    pub fn fast_eq(lhs: &Self, rhs: &Self) -> bool {
        Rc::ptr_eq(&lhs.inner, &rhs.inner)
            || lhs
                .as_biguint()
                .and_then(|lhs| rhs.as_biguint().map(|rhs| lhs == rhs))
                .unwrap_or(false)
    }

    fn as_biguint(&self) -> Option<&BigUint> {
        match &*self.inner {
            IntInner::Mul(ratio) => ratio.as_biguint(),
            IntInner::Min(emin) => emin.as_biguint(),
            IntInner::Lcm(elcm) => elcm.as_biguint(),
        }
    }

    pub fn to_u32(&self) -> Option<u32> {
        match &*self.inner {
            IntInner::Min(emin) => emin.to_u32(),
            IntInner::Lcm(elcm) => elcm.to_u32(),
            IntInner::Mul(ratio) => ratio.to_u32(),
        }
    }
}

impl<P> Range for Int<P>
where
    P: Atom,
{
    fn min_value(&self) -> u64 {
        match &*self.inner {
            IntInner::Lcm(elcm) => elcm.gcd.to_u64().unwrap(),
            IntInner::Min(emin) => emin.min.to_u64().unwrap(),
            IntInner::Mul(ratio) => ratio.min_value(),
        }
    }

    fn max_value(&self) -> u64 {
        match &*self.inner {
            IntInner::Lcm(elcm) => elcm.lcm.to_u64().unwrap(),
            IntInner::Min(emin) => emin.max.to_u64().unwrap(),
            IntInner::Mul(ratio) => ratio.max_value(),
        }
    }
}

impl<'a, 'b, P> ops::Mul<&'a BigUint> for &'b Int<P>
where
    P: Atom,
{
    type Output = Int<P>;

    fn mul(self, other: &'a BigUint) -> Int<P> {
        if other.is_one() {
            self.clone()
        } else if other.is_zero() {
            0u32.into()
        } else if let Some(val) = self.as_biguint() {
            (val * other).into()
        } else {
            use IntInner::*;

            match &*self.inner {
                Lcm(lcm) => Lcm(lcm * other).into(),
                Min(min) => Min(min * other).into(),
                Mul(ratio) => (ratio * other).into(),
            }
        }
    }
}

impl<P> ops::MulAssign<&'_ BigUint> for Int<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ BigUint) {
        if other.is_one() {
            // Nothing to do
        } else if other.is_zero() {
            *self = 0u32.into()
        } else if let Some(val) = self.as_biguint() {
            *self = (val * other).into()
        } else {
            use IntInner::*;

            match Rc::make_mut(&mut self.inner) {
                Lcm(lcm) => *lcm *= other,
                Min(min) => *min *= other,
                Mul(ratio) => *ratio *= other,
            }
        }
    }
}

forward_binop!(impl(P: Atom) Mul, mul for Int<P>, BigUint, MulAssign, mul_assign);

impl<'a, 'b, P> ops::Mul<&'a Ratio<P>> for &'b Int<P>
where
    P: Atom,
{
    type Output = Int<P>;

    fn mul(self, other: &'a Ratio<P>) -> Int<P> {
        if let Some(val) = other.as_biguint() {
            self * val
        } else if let Some(val) = self.as_biguint() {
            (other * val).into()
        } else {
            match &*self.inner {
                IntInner::Mul(lhs) => (lhs * other).into(),
                IntInner::Lcm(lhs) => (lhs * other).into(),
                IntInner::Min(lhs) => (lhs * other).into(),
            }
        }
    }
}

impl<P> ops::MulAssign<&'_ Ratio<P>> for Int<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ Ratio<P>) {
        if let Some(val) = other.as_biguint() {
            *self *= val;
        } else if let Some(val) = self.as_biguint() {
            *self = (other * val).into();
        } else {
            use IntInner::*;

            match Rc::make_mut(&mut self.inner) {
                Mul(lhs) => *lhs *= other,
                Lcm(lhs) => *lhs *= other,
                Min(lhs) => *lhs *= other,
            }
        }
    }
}

forward_binop!(impl(P: Atom) Mul, mul for Int<P>, Ratio<P>, MulAssign, mul_assign);

impl<'a, 'b, P> ops::Mul<&'a Int<P>> for &'b Int<P>
where
    P: Atom,
{
    type Output = Int<P>;

    fn mul(self, other: &'a Int<P>) -> Int<P> {
        if let Some(val) = other.as_biguint() {
            self * val
        } else if let Some(val) = self.as_biguint() {
            other * val
        } else {
            use IntInner::*;

            match (&*self.inner, &*other.inner) {
                (_, Mul(rhs)) => self * rhs,
                (Mul(lhs), _) => other * lhs,
                _ => unimplemented!("int mul of {} and {}", self, other),
            }
        }
    }
}

impl<P> ops::MulAssign<&'_ Int<P>> for Int<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, other: &'_ Int<P>) {
        if let Some(val) = other.as_biguint() {
            *self *= val;
        } else if let Some(val) = self.as_biguint() {
            *self = other * val;
        } else {
            use IntInner::*;

            match &*other.inner {
                Mul(other) => *self *= other,
                _ => {
                    *self = match &*self.inner {
                        Mul(lhs) => other * lhs,
                        _ => unimplemented!("int mul of {} and {}", self, other),
                    }
                }
            }
        }
    }
}

forward_binop_commutative!(impl(P: Atom) Mul, mul for Int<P>, Int<P>, MulAssign, mul_assign);

impl<'a, P> ops::MulAssign<&'a u64> for Int<P>
where
    P: Atom,
{
    #[inline]
    fn mul_assign(&mut self, rhs: &'a u64) {
        *self *= BigUint::from(*rhs);
    }
}

forward_binop_to_op_assign!(impl(P: Atom) Mul, mul for Int<P>, u64, MulAssign, mul_assign);

impl<'a, P: 'a> iter::Product<&'a Int<P>> for Int<P>
where
    P: Atom,
{
    fn product<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a Int<P>>,
    {
        let mut res = Self::one();
        for elem in iter {
            res *= elem;
        }
        res
    }
}

impl<P> iter::Product<Int<P>> for Int<P>
where
    P: Atom,
{
    fn product<I>(mut iter: I) -> Self
    where
        I: Iterator<Item = Int<P>>,
    {
        if let Some(mut res) = iter.next() {
            for elem in iter {
                res *= elem;
            }
            res
        } else {
            Self::one()
        }
    }
}

// factor * float(numer) / float(denom)
#[derive(Debug, Clone, PartialEq, PartialOrd)]
struct FloatRatioInner<P> {
    factor: f64,
    numer: Vec<P>,
    denom: Vec<P>,
    // should be: factor * float(numer/denom) * float(numer)/float(denom)
}

impl<P> From<Ratio<P>> for FloatRatioInner<P>
where
    P: Atom,
{
    fn from(ratio: Ratio<P>) -> Self {
        // TODO: This is no longer an integer ratio!
        FloatRatioInner {
            factor: ratio.inner.factor.to_u64().unwrap() as f64,
            numer: ratio.inner.numer.clone(),
            denom: ratio.inner.denom.clone(),
        }
    }
}

impl<P> Eq for FloatRatioInner<P> where P: Eq {}

impl<P> Ord for FloatRatioInner<P>
where
    P: Ord,
{
    fn cmp(&self, other: &FloatRatioInner<P>) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl<P> Hash for FloatRatioInner<P>
where
    P: Hash,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.factor.to_bits().hash(state);
        self.numer.hash(state);
        self.denom.hash(state);
    }
}

impl<'a, P> MulAssign<&'a FloatRatioInner<P>> for FloatRatioInner<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, rhs: &'a FloatRatioInner<P>) {
        self.factor *= &rhs.factor;
        for numer in rhs.numer.iter() {
            if let Some(pos) = self.denom.iter().position(|d| d == numer) {
                self.denom.swap_remove(pos);
            } else {
                self.numer.push(numer.clone());
            }
        }
        for denom in rhs.denom.iter() {
            if let Some(pos) = self.numer.iter().position(|n| n == denom) {
                self.numer.swap_remove(pos);
            } else {
                self.denom.push(denom.clone());
            }
        }

        self.numer.sort();
        self.denom.sort();
    }
}

forward_binop_to_op_assign_commutative!(impl(P: Atom) Mul, mul for FloatRatioInner<P>, FloatRatioInner<P>, MulAssign, mul_assign);

impl<P> fmt::Display for FloatRatioInner<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.numer.is_empty() {
            write!(fmt, "{}", self.factor)?;
        } else {
            if !self.factor.is_one() {
                write!(fmt, "{}*", self.factor)?;
            }

            write!(fmt, "{}", self.numer.iter().format("*"))?;
        }

        if !self.denom.is_empty() {
            write!(fmt, "/{}", self.denom.iter().format("/"))?;
        }

        Ok(())
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FloatRatio<P> {
    inner: Rc<FloatRatioInner<P>>,
}

impl<P> From<Ratio<P>> for FloatRatio<P>
where
    P: Atom,
{
    fn from(ratio: Ratio<P>) -> Self {
        FloatRatio {
            inner: Rc::new(ratio.into()),
        }
    }
}

impl<P> Clone for FloatRatio<P> {
    fn clone(&self) -> Self {
        FloatRatio {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<'a, P> MulAssign<&'a FloatRatio<P>> for FloatRatio<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, rhs: &'a FloatRatio<P>) {
        *Rc::make_mut(&mut self.inner) *= &*rhs.inner;
    }
}

forward_binop_to_op_assign_commutative!(impl(P: Atom) Mul, mul for FloatRatio<P>, FloatRatio<P>, MulAssign, mul_assign);

impl<P> fmt::Debug for FloatRatio<P>
where
    P: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, fmt)
    }
}

impl<P> fmt::Display for FloatRatio<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.inner, fmt)
    }
}

impl<P> From<FloatRatioInner<P>> for FloatRatio<P>
where
    P: Atom,
{
    fn from(inner: FloatRatioInner<P>) -> Self {
        FloatRatio {
            inner: Rc::new(inner),
        }
    }
}

impl<P> From<f64> for FloatRatioInner<P>
where
    P: Atom,
{
    fn from(constant: f64) -> Self {
        FloatRatioInner {
            factor: constant,
            numer: Vec::new(),
            denom: Vec::new(),
        }
    }
}

impl<P> From<f64> for FloatRatio<P>
where
    P: Atom,
{
    fn from(constant: f64) -> Self {
        FloatRatioInner::from(constant).into()
    }
}

impl<P> FloatRatio<P>
where
    P: Atom,
{
    fn new_constant(factor: f64) -> Self {
        FloatRatio::new(factor, Vec::new(), Vec::new())
    }

    fn new(factor: f64, numer: Vec<P>, denom: Vec<P>) -> Self {
        FloatRatioInner {
            factor,
            numer,
            denom,
        }
        .into()
    }

    fn is_one(&self) -> bool {
        self.inner.factor == 1f64
            && self.inner.numer.is_empty()
            && self.inner.denom.is_empty()
    }

    fn as_f64(&self) -> Option<f64> {
        if self.inner.numer.is_empty() && self.inner.denom.is_empty() {
            Some(self.inner.factor)
        } else {
            None
        }
    }

    fn to_f64(&self) -> Option<f64> {
        self.as_f64()
    }

    fn min_value(&self) -> f64
    where
        P: Atom,
    {
        let min = self.inner.factor;
        let min = self
            .inner
            .numer
            .iter()
            .fold(min, |min, n| min * n.min_value() as f64);
        self.inner
            .denom
            .iter()
            .fold(min, |min, d| min / d.max_value() as f64)
    }

    fn max_value(&self) -> f64
    where
        P: Atom,
    {
        let max = self.inner.factor;
        let max = self
            .inner
            .numer
            .iter()
            .fold(max, |max, n| max * n.max_value() as f64);
        self.inner
            .denom
            .iter()
            .fold(max, |max, d| max / d.min_value() as f64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffExpr<P>
where
    P: Atom,
{
    constant: f64,
    //ratios: WeightedVecSet<FloatRatio<P>>,
    values: WeightedVecSet<Float<P>>,
}

impl<P> Eq for DiffExpr<P> where P: Atom + Eq {}

/*
impl<P> PartialOrd for DiffExpr<P>
where
    P: Atom,
{
    fn partial_cmp(&self, other: &DiffExpr<P>) -> Option<Ordering> {
        let mut seen_lt = false;
        let mut seen_gt = false;

        if !is_close(self.constant, other.constant) {
            if self.constant < other.constant {
                seen_lt = true;
            } else {
                seen_gt = true;
            }
        }

        for (self_weight, other_weight, _) in
            WeightedZip::new(self.values.iter(), other.values.iter())
        {
            if !is_close(self_weight, other_weight) {
                if self_weight < other_weight {
                    if seen_gt {
                        return None;
                    }
                    seen_lt = true;
                } else {
                    if seen_lt {
                        return None;
                    }
                    seen_gt = true;
                }
            }
        }

        assert!(!seen_lt || !seen_gt);

        Some(if seen_lt {
            Ordering::Less
        } else if seen_gt {
            Ordering::Greater
        } else {
            Ordering::Equal
        })
    }
}
*/

impl<P> PartialOrd for DiffExpr<P>
where
    P: Atom + PartialOrd,
{
    fn partial_cmp(&self, other: &DiffExpr<P>) -> Option<Ordering> {
        self.values.partial_cmp(&other.values).map(|ord| {
            ord.then_with(|| self.constant.partial_cmp(&other.constant).unwrap())
        })
    }
}

impl<P> Ord for DiffExpr<P>
where
    P: Atom + Ord,
{
    fn cmp(&self, other: &DiffExpr<P>) -> Ordering {
        self.values
            .cmp(&other.values)
            .then_with(|| self.constant.partial_cmp(&other.constant).unwrap())
    }
}

impl<P> Hash for DiffExpr<P>
where
    P: Atom,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.constant.to_bits().hash(state);
        self.values.hash(state);
    }
}

impl<P> fmt::Display for DiffExpr<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        if self.values.is_empty() {
            write!(fmt, "{}", self.constant)
        } else {
            write!(fmt, "(")?;

            let (positive, negative) = self
                .values
                .iter()
                .partition::<Vec<_>, _>(|(weight, _)| weight > &0.);

            let mut has_written = false;
            if !self.constant.is_zero() {
                write!(fmt, "{}", self.constant)?;

                has_written = true;
            }

            if !positive.is_empty() {
                write!(
                    fmt,
                    "{}{}",
                    if has_written { " + " } else { "" },
                    positive
                        .iter()
                        .map(|(weight, item)| format!("{}*{}", weight, item))
                        .format(" + "),
                )?;

                has_written = true;
            }

            if !negative.is_empty() {
                write!(
                    fmt,
                    "{}{}",
                    if has_written { " - " } else { "-" },
                    negative
                        .iter()
                        .map(|(weight, item)| format!("{}*{}", -weight, item))
                        .format(" - ")
                )?;
            }

            write!(fmt, ")")?;

            Ok(())
        }
    }
}

impl<'a, 'b, P> ops::Add<&'a f64> for &'b DiffExpr<P>
where
    P: Atom,
{
    type Output = DiffExpr<P>;

    fn add(self, other: &'a f64) -> DiffExpr<P> {
        let result = DiffExpr {
            constant: self.constant + other,
            values: self.values.clone(),
        };

        #[cfg(verify)]
        {
            assert!(is_close(self.min_value() + other, result.min_value()));
            assert!(is_close(self.max_value() + other, result.max_value()));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Add<Output = DiffExpr<P>>, add for DiffExpr<P>, f64);

impl<'a, 'b, P> ops::Add<&'a DiffExpr<P>> for &'b DiffExpr<P>
where
    P: Atom,
{
    type Output = DiffExpr<P>;

    fn add(self, other: &'a DiffExpr<P>) -> DiffExpr<P> {
        let result = DiffExpr {
            constant: self.constant + other.constant,
            values: self.values.union(&other.values),
        };

        #[cfg(verify)]
        {
            assert!(is_close(
                self.min_value() + other.min_value(),
                result.min_value()
            ));
            assert!(is_close(
                self.max_value() + other.max_value(),
                result.max_value()
            ));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Add<Output = DiffExpr<P>>, add for DiffExpr<P>, DiffExpr<P>);

impl<'a, 'b, P> ops::Sub<&'a f64> for &'b DiffExpr<P>
where
    P: Atom,
{
    type Output = DiffExpr<P>;

    fn sub(self, other: &'a f64) -> DiffExpr<P> {
        let result = DiffExpr {
            constant: self.constant - other,
            values: self.values.clone(),
        };

        #[cfg(verify)]
        {
            assert!(is_close(self.min_value() - other, result.min_value()));
            assert!(is_close(self.max_value() - other, result.max_value()));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Sub<Output = DiffExpr<P>>, sub for DiffExpr<P>, f64);

impl<'a, 'b, P> ops::Sub<&'a DiffExpr<P>> for &'b DiffExpr<P>
where
    P: Atom,
{
    type Output = DiffExpr<P>;

    fn sub(self, other: &'a DiffExpr<P>) -> DiffExpr<P> {
        let result = DiffExpr {
            constant: self.constant - other.constant,
            values: self.values.union_map(&other.values, |weight| -weight),
        };

        #[cfg(verify)]
        {
            assert!(is_close(
                self.min_value() - other.max_value(),
                result.min_value()
            ));
            assert!(is_close(
                self.max_value() - other.min_value(),
                result.max_value()
            ));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Sub<Output = DiffExpr<P>>, sub for DiffExpr<P>, DiffExpr<P>);

impl<'a, 'b, P> ops::Mul<&'a f64> for &'b DiffExpr<P>
where
    P: Atom,
{
    type Output = DiffExpr<P>;

    fn mul(self, other: &'a f64) -> DiffExpr<P> {
        let result = DiffExpr {
            constant: self.constant * other,
            values: self.values.map_weights(|weight| weight * other),
        };

        #[cfg(verify)]
        {
            if other > 0. {
                assert!(is_close(self.min_value() * other, result.min_value()));
                assert!(is_close(self.max_value() * other, result.max_value()));
            } else {
                assert!(is_close(self.min_value() * other, result.max_value()));
                assert!(is_close(self.max_value() * other, result.min_value()));
            }
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Mul<Output = DiffExpr<P>>, mul for DiffExpr<P>, f64);

impl<'a, 'b, P> ops::Mul<&'a FloatRatio<P>> for &'b DiffExpr<P>
where
    P: Atom,
{
    type Output = DiffExpr<P>;

    fn mul(self, other: &'a FloatRatio<P>) -> DiffExpr<P> {
        if let Some(value) = other.as_f64() {
            self * value
        } else {
            let factor = other.inner.factor;
            let naked = Float::ratio(
                1f64,
                other.inner.numer.clone(),
                other.inner.denom.clone(),
                Vec::new(),
            );
            DiffExpr {
                constant: 0f64,
                values: if self.constant.is_zero() {
                    None
                } else {
                    Some((factor * self.constant, naked.clone()))
                }
                .into_iter()
                .chain(
                    self.values
                        .iter()
                        .map(|(weight, item)| (weight * factor, item * &naked)),
                )
                .collect(),
            }
        }
    }
}

impl<P> From<f64> for DiffExpr<P>
where
    P: Atom,
{
    fn from(constant: f64) -> Self {
        DiffExpr {
            constant,
            values: Default::default(),
        }
    }
}

impl<P> From<FloatRatio<P>> for DiffExpr<P>
where
    P: Atom,
{
    fn from(ratio: FloatRatio<P>) -> Self {
        match ratio.as_f64() {
            Some(value) => value.into(),
            None => DiffExpr {
                constant: 0.,
                values: WeightedVecSet::from_sorted_iter(iter::once((
                    ratio.inner.factor,
                    FloatInner::Mul(
                        FloatRatio::new(
                            1.,
                            ratio.inner.numer.clone(),
                            ratio.inner.denom.clone(),
                        ),
                        Vec::new(),
                    )
                    .into(),
                ))),
            },
        }
    }
}

impl<P> From<Float<P>> for DiffExpr<P>
where
    P: Atom,
{
    fn from(value: Float<P>) -> Self {
        match value.as_f64() {
            Some(val) => DiffExpr {
                constant: val,
                values: Default::default(),
            },
            None => match &*value {
                FloatInner::Diff(ediff) => ediff.clone(),
                FloatInner::Mul(ratio, args) => DiffExpr {
                    constant: 0.,
                    values: WeightedVecSet::from_sorted_iter(iter::once((
                        ratio.inner.factor,
                        FloatInner::Mul(
                            FloatRatio::new(
                                1.,
                                ratio.inner.numer.clone(),
                                ratio.inner.denom.clone(),
                            ),
                            args.clone(),
                        )
                        .into(),
                    ))),
                },
                _ => DiffExpr {
                    constant: 0.,
                    values: WeightedVecSet::from_sorted_iter(iter::once((1., value))),
                },
            },
        }
    }
}

impl<P> DiffExpr<P>
where
    P: Atom,
{
    fn min_value(&self) -> f64
    where
        P: Atom,
    {
        self.constant
            + self
                .values
                .iter()
                .map(|(weight, value)| {
                    if weight < 0. {
                        weight * value.max_value()
                    } else {
                        weight * value.min_value()
                    }
                })
                .sum::<f64>()
    }

    fn max_value(&self) -> f64
    where
        P: Atom,
    {
        self.constant
            + self
                .values
                .iter()
                .map(|(weight, value)| {
                    if weight < 0. {
                        weight * value.min_value()
                    } else {
                        weight * value.max_value()
                    }
                })
                .sum::<f64>()
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct FMaxExpr<P>
where
    P: Atom,
{
    min: f64,
    max: f64,
    values: VecSet<DiffExpr<P>>,
}

impl<P> Eq for FMaxExpr<P> where P: Atom + Eq {}

impl<P> Ord for FMaxExpr<P>
where
    P: Atom + Ord,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl<P> Hash for FMaxExpr<P>
where
    P: Atom + Hash,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.values.hash(state);
    }
}

impl<P> fmt::Display for FMaxExpr<P>
where
    P: Atom,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "max[{} <= {}](", self.min, self.max)?;

        if self.values.is_empty() {
            debug_assert_eq!(self.min, self.max);

            write!(fmt, "{}", self.min)?;
        } else {
            let maxmin = self
                .values
                .iter()
                .map(DiffExpr::min_value)
                .fold(std::f64::NEG_INFINITY, f64::max);
            assert!(maxmin.is_finite());

            if maxmin != self.min {
                write!(fmt, "{}, ", self.min)?;
            }

            write!(
                fmt,
                "{}",
                self.values
                    .iter()
                    .map(|val| format!(
                        "[{} <= {}] {}",
                        val.min_value(),
                        val.max_value(),
                        val
                    ))
                    .format(", ")
            )?;
        }

        write!(fmt, ")")
    }
}

impl<P> From<FMaxExpr<P>> for Float<P>
where
    P: Atom,
{
    fn from(emax: FMaxExpr<P>) -> Self {
        emax.simplified()
            .unwrap_or_else(|emax| FloatInner::Max(emax).into())
    }
}

impl<P> From<f64> for FMaxExpr<P>
where
    P: Atom,
{
    fn from(constant: f64) -> Self {
        Self::new(constant, constant, VecSet::default())
    }
}

impl<P> From<Float<P>> for FMaxExpr<P>
where
    P: Atom,
{
    fn from(value: Float<P>) -> Self {
        match value.as_f64() {
            Some(val) => val.into(),
            None => match &*value {
                FloatInner::Max(emax) => emax.clone(),
                _ => FMaxExpr::new(
                    value.min_value(),
                    value.max_value(),
                    VecSet::new(DiffExpr::from(value)),
                ),
            },
        }
    }
}

impl<'a, 'b, P> ops::Add<&'a f64> for &'b FMaxExpr<P>
where
    P: Atom,
{
    type Output = FMaxExpr<P>;

    fn add(self, other: &'a f64) -> FMaxExpr<P> {
        FMaxExpr::new(
            self.min + other,
            self.max + other,
            self.values.iter().map(|item| item + other).collect(),
        )
    }
}

impl<'a, 'b, P> ops::Mul<&'a f64> for &'b FMaxExpr<P>
where
    P: Atom,
{
    type Output = FMaxExpr<P>;

    fn mul(self, other: &'a f64) -> FMaxExpr<P> {
        assert!(*other > 0.);

        FMaxExpr::new(
            self.min * other,
            self.max * other,
            self.values.iter().map(|item| item * other).collect(),
        )
    }
}

impl<'a, 'b, P> ops::Mul<&'a FloatRatio<P>> for &'b FMaxExpr<P>
where
    P: Atom,
{
    type Output = FMaxExpr<P>;

    fn mul(self, other: &'a FloatRatio<P>) -> FMaxExpr<P> {
        assert!(other.min_value() > 0.);

        let args: VecSet<_> = self.values.iter().map(|item| item * other).collect();
        let min = args
            .iter()
            .map(DiffExpr::min_value)
            .fold(self.min * other.min_value(), f64::max);
        let max = args
            .iter()
            .map(DiffExpr::max_value)
            .fold(self.max * other.max_value(), f64::max);

        FMaxExpr::new(min, max, args)
    }
}

impl<P> FMaxExpr<P>
where
    P: Atom,
{
    fn new(min: f64, max: f64, values: VecSet<DiffExpr<P>>) -> Self {
        assert!(min.is_finite() && max.is_finite());
        assert!(min <= max);

        FMaxExpr { min, max, values }
    }

    fn fmax(&self, other: &FMaxExpr<P>) -> FMaxExpr<P> {
        if gt_close(self.min, other.max) {
            self.clone()
        } else if gt_close(other.min, self.max) {
            other.clone()
        } else {
            let mut self_to_remove = Vec::with_capacity(self.values.len());
            let mut other_to_remove = Vec::with_capacity(other.values.len());

            let min = self.min.max(other.min);
            let max = self.max.max(other.max);

            for (ix, value) in other.values.iter().enumerate() {
                if value.max_value() <= min {
                    other_to_remove.push(ix);
                }
            }

            for (six, lhs) in self.values.iter().enumerate() {
                if lhs.max_value() <= min {
                    self_to_remove.push(six);
                    continue;
                }

                let mut orp = 0;
                for (oix, rhs) in other.values.iter().enumerate() {
                    if orp < other_to_remove.len() && oix == other_to_remove[orp] {
                        orp += 1;
                        continue;
                    }

                    let diff = lhs - rhs;
                    if gt_close(diff.min_value(), 0.) {
                        other_to_remove.insert(orp, oix);
                        orp += 1;
                    } else if lt_close(diff.max_value(), 0.) {
                        self_to_remove.push(six);
                    }
                }
            }

            let values = if self_to_remove.is_empty()
                && other_to_remove.len() == other.values.len()
            {
                self.values.clone()
            } else if other_to_remove.is_empty()
                && self_to_remove.len() == self.values.len()
            {
                other.values.clone()
            } else {
                VecSet::from_sorted_iter(
                    Union::new(
                        self.values
                            .iter()
                            .enumerate()
                            .filter(|(ix, _)| !self_to_remove.contains(ix))
                            .map(|(_, value)| value),
                        other
                            .values
                            .iter()
                            .enumerate()
                            .filter(|(ix, _)| !other_to_remove.contains(ix))
                            .map(|(_, value)| value),
                    )
                    .cloned(),
                )
            };

            FMaxExpr::new(min, max, values)
        }
    }

    fn is_constant(&self) -> bool {
        self.min == self.max
    }

    fn is_single_value(&self) -> bool {
        // Need to check that the min value is close to the min value of the single arg, because it
        // is possible that we computed a min with a constant value
        self.values.len() == 1 && is_close(self.values[0].min_value(), self.min)
    }

    fn simplified(self) -> Result<Float<P>, Self> {
        if self.is_constant() {
            debug_assert_eq!(self.min, self.max);

            Ok(self.min.into())
        } else if self.is_single_value() {
            debug_assert!(self.values.len() == 1);

            Ok(self.values.values[0].clone().into())
        } else {
            Err(self)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FloatInner<P>
where
    P: Atom,
{
    /// Product of all arguments.  We keep the `FloatRatio` separate to make simplifications
    /// easier.
    Mul(FloatRatio<P>, Vec<Float<P>>),
    Min(FMinExpr<P>),
    Max(FMaxExpr<P>),
    Diff(DiffExpr<P>),
    // Ceil division: ceil(numer / denom)
    DivCeil(Int<P>, u32),
}

impl<P> fmt::Display for FloatInner<P>
where
    P: Atom + fmt::Display,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use FloatInner::*;

        match self {
            DivCeil(numer, denom) => write!(fmt, "div_ceil({}, {})", numer, denom),
            Mul(ratio, args) if args.is_empty() => write!(fmt, "{}", ratio),
            Mul(ratio, args) => {
                if !ratio.is_one() {
                    write!(fmt, "{}*", ratio)?;
                }

                write!(fmt, "{}", args.iter().format("*"))
            }
            Min(emin) => write!(fmt, "{}", emin),
            Max(emax) => write!(fmt, "{}", emax),
            Diff(diff) => write!(fmt, "{}", diff),
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
pub struct Float<P>
where
    P: Atom,
{
    inner: Rc<hash::MemoizedHash<FloatInner<P>>>,
}

impl<P> PartialOrd for Float<P>
where
    P: Atom + PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if Rc::ptr_eq(&self.inner, &other.inner) {
            Some(Ordering::Equal)
        } else {
            self.inner.partial_cmp(&other.inner)
        }
    }
}

impl<P> Ord for Float<P>
where
    P: Atom + Ord,
{
    fn cmp(&self, other: &Self) -> Ordering {
        if Rc::ptr_eq(&self.inner, &other.inner) {
            Ordering::Equal
        } else {
            self.inner.cmp(&other.inner)
        }
    }
}

impl<P> AsRef<FloatInner<P>> for Float<P>
where
    P: Atom,
{
    fn as_ref(&self) -> &FloatInner<P> {
        self
    }
}

impl<P> ops::Deref for Float<P>
where
    P: Atom,
{
    type Target = FloatInner<P>;

    fn deref(&self) -> &FloatInner<P> {
        &**self.inner
    }
}

impl<P> fmt::Debug for Float<P>
where
    P: Atom,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, fmt)
    }
}

impl<P> fmt::Display for Float<P>
where
    P: Atom,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.inner, fmt)
    }
}

impl<P> Clone for Float<P>
where
    P: Atom,
{
    fn clone(&self) -> Self {
        Float {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<P> From<DiffExpr<P>> for Float<P>
where
    P: Atom,
{
    fn from(diff: DiffExpr<P>) -> Self {
        if diff.values.is_empty() {
            diff.constant.into()
        } else if diff.values.len() == 1 && diff.constant == 0. {
            let weight = diff.values.weights[0];
            let item = diff.values.coefficients[0].clone();
            item * weight
        } else {
            FloatInner::Diff(diff).into()
        }
    }
}

impl<P> From<FloatInner<P>> for Float<P>
where
    P: Atom,
{
    fn from(inner: FloatInner<P>) -> Self {
        Float {
            inner: Rc::new(hash::MemoizedHash::new(inner)),
        }
    }
}

impl<P> From<f64> for Float<P>
where
    P: Atom,
{
    fn from(constant: f64) -> Self {
        FloatInner::Mul(constant.into(), Vec::new()).into()
    }
}

impl<P> Float<P>
where
    P: Atom,
{
    fn ratio(factor: f64, numer: Vec<P>, denom: Vec<P>, args: Vec<Float<P>>) -> Self {
        FloatInner::Mul(
            FloatRatio::from(FloatRatioInner {
                factor,
                numer,
                denom,
            }),
            args,
        )
        .into()
    }

    pub fn div_ceil(lhs: &Int<P>, rhs: u32) -> Self {
        if lhs.max_value() <= u64::from(rhs) {
            assert!(lhs.min_value() > 0);

            1f64.into()
        } else {
            match (&*lhs.inner, lhs.as_biguint()) {
                (_, Some(value)) => {
                    Int::from((value + rhs - 1u32) / rhs).to_symbolic_float()
                }
                // TODO: This should be a check on factors!
                (IntInner::Mul(ratio), None)
                    if ratio.gcd_value().is_multiple_of(&rhs.into()) =>
                {
                    Float::ratio(
                        ratio.inner.factor.to_u64().unwrap() as f64 / f64::from(rhs),
                        ratio.inner.numer.clone(),
                        ratio.inner.denom.clone(),
                        Vec::new(),
                    )
                }
                (_, None) => FloatInner::DivCeil(lhs.clone(), rhs).into(),
            }
        }
    }

    fn is_one(&self) -> bool {
        match &**self {
            FloatInner::Mul(ratio, args) if args.is_empty() => ratio.is_one(),
            _ => false,
        }
    }

    fn is_zero(&self) -> bool {
        self.as_f64().map(|x| x.is_zero()).unwrap_or(false)
    }

    pub fn min(&self, other: &Float<P>) -> Float<P> {
        trace!("min lhs: {}", self);
        trace!("min rhs: {}", other);

        let result = FMinExpr::from(self.clone())
            .fmin(&FMinExpr::from(other.clone()))
            .into();

        trace!("min out: {}", result);
        result
    }

    pub fn min_assign(&mut self, other: &Float<P>) {
        *self = Float::min(&self.clone(), other);
    }

    pub fn max(&self, other: &Float<P>) -> Float<P> {
        trace!("max lhs: {}", self);
        trace!("max rhs: {}", other);

        let result = FMaxExpr::from(self.clone())
            .fmax(&FMaxExpr::from(other.clone()))
            .into();

        trace!("max out: {}", result);
        result
    }

    pub fn max_assign(&mut self, other: &Float<P>) {
        *self = Float::max(&self.clone(), other);
    }

    pub fn as_f64(&self) -> Option<f64> {
        match &**self {
            FloatInner::Mul(ratio, args) if args.is_empty() => ratio.as_f64(),
            _ => None,
        }
    }

    pub fn to_f64(&self) -> Option<f64> {
        match &**self {
            FloatInner::DivCeil(lhs, rhs) => {
                lhs.to_u32().map(|lhs| ((lhs + rhs - 1) / rhs) as f64)
            }
            FloatInner::Mul(ratio, args) => ratio.to_f64().and_then(|ratio| {
                args.iter()
                    .map(|arg| arg.to_f64().ok_or(()))
                    .product::<Result<f64, ()>>()
                    .ok()
                    .map(|result| ratio * result)
            }),
            FloatInner::Max(emax) => unimplemented!("to_f64 for {}", emax),
            FloatInner::Min(emin) => unimplemented!("to_f64 for {}", emin),
            FloatInner::Diff(diff) => unimplemented!("to_f64 for {}", diff),
        }
    }

    pub fn min_value(&self) -> f64 {
        info!("min_value for {}", self);

        match &**self {
            FloatInner::DivCeil(numer, denom) => {
                let numer_min = numer.min_value();
                let denom = u64::from(*denom);
                // TODO: should take gcd for le min
                ((numer_min + denom - 1) / denom) as f64
            }
            FloatInner::Mul(ratio, args) => {
                // Find the DivCeils and collect theirs factors
                let (divceils, others) = args.iter().fold(
                    (
                        Vec::with_capacity(args.len()),
                        Vec::with_capacity(args.len()),
                    ),
                    |(mut divceils, mut others), arg| {
                        match &**arg {
                            FloatInner::DivCeil(numer, denom) => match &*numer.inner {
                                IntInner::Mul(ratio) => divceils.push((
                                    &ratio.inner.factor,
                                    &ratio.inner.numer,
                                    &ratio.inner.denom,
                                    denom,
                                )),
                                _ => unimplemented!("min_value containing {}", arg),
                            },
                            _ => others.push(arg),
                        };
                        (divceils, others)
                    },
                );

                let min = ratio.inner.factor.clone();

                let min = if divceils.len() == 1 {
                    let (dcfactor, dcnumer, dcdenom, dcdivisor) = divceils[0];

                    // numer is only on the ratio numerator
                    let mut numer = ratio.inner.numer.iter().collect::<Vec<_>>();
                    // denom is only on the ratio denominator
                    let mut denom = ratio.inner.denom.iter().collect::<Vec<_>>();
                    // dconly is only on the lcm numer
                    let mut dc_numer_value = dcfactor.clone();
                    let mut dc_denom_value = BigUint::from(*dcdivisor);

                    let mut min = min;
                    for dcn in dcnumer {
                        if let Some(pos) = denom.iter().position(|elem| *elem == dcn) {
                            // Don't use it twice!
                            denom.swap_remove(pos);

                            let lcm_value = dcn.lcm_value();
                            dc_numer_value *= lcm_value;
                            min /= lcm_value as f64;
                        } else {
                            dc_numer_value *= dcn.gcd_value();
                        }
                    }

                    for dcd in dcdenom {
                        if let Some(pos) = numer.iter().position(|elem| *elem == dcd) {
                            // Don't use it twice!
                            numer.swap_remove(pos);

                            let gcd_value = dcd.gcd_value();
                            dc_denom_value *= gcd_value;
                            min *= gcd_value as f64;
                        } else {
                            dc_denom_value *= dcd.lcm_value();
                        }
                    }

                    let min = min
                        * ((dc_numer_value + &dc_denom_value - 1u32) / dc_denom_value)
                            .to_u64()
                            .unwrap() as f64;

                    let min = numer
                        .into_iter()
                        .fold(min, |min, num| min * num.min_value() as f64);

                    denom
                        .into_iter()
                        .fold(min, |min, denom| min / denom.max_value() as f64)

                // dcnumer_only -> min (gcd?)
                // common -> lcm
                // denom_only -> max
                } else {
                    let min = divceils.into_iter().fold(
                        min,
                        |min, (dcfactor, dcnumer, dcdenom, dcdivisor)| {
                            let dcnumer = dcnumer
                                .into_iter()
                                .fold(dcfactor.clone(), |factor, num| {
                                    factor * num.gcd_value()
                                });
                            let dcdenom = dcdenom
                                .into_iter()
                                .fold(BigUint::from(*dcdivisor), |div, denom| {
                                    div * denom.lcm_value()
                                });
                            min * ((&dcnumer + &dcdenom - 1u32) / dcdenom)
                                .to_u64()
                                .unwrap() as f64
                        },
                    );

                    let min = ratio
                        .inner
                        .numer
                        .iter()
                        .fold(min, |min, num| min * num.min_value() as f64);

                    ratio
                        .inner
                        .denom
                        .iter()
                        .fold(min, |min, denom| min / denom.max_value() as f64)
                };

                others
                    .into_iter()
                    .fold(min, |min, other| min * other.min_value())
            }
            FloatInner::Max(emax) => emax.min,
            FloatInner::Min(emin) => emin.min,
            FloatInner::Diff(diff) => diff.min_value(),
        }
    }

    pub fn max_value(&self) -> f64 {
        info!("max_value for {}", self);

        match &**self {
            FloatInner::DivCeil(numer, denom) => {
                let denom = u64::from(*denom);
                ((numer.max_value() + denom - 1) / denom) as f64
            }
            FloatInner::Mul(ratio, args) => {
                let mut max = ratio.max_value();
                for arg in args.iter() {
                    max *= arg.max_value();
                }
                max
            }
            FloatInner::Max(emax) => emax.max,
            FloatInner::Min(emin) => emin.max,
            FloatInner::Diff(diff) => diff.max_value(),
        }
    }
}

impl<P> MulAssign<&'_ Float<P>> for Float<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, rhs: &'_ Float<P>) {
        *self = ops::Mul::mul(&self.clone(), rhs);
    }
}

impl<P> MulAssign<Float<P>> for Float<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, rhs: Float<P>) {
        *self = ops::Mul::mul(&self.clone(), rhs);
    }
}

impl<'a, 'b, P> Mul<&'a f64> for &'b Float<P>
where
    P: Atom,
{
    type Output = Float<P>;

    fn mul(self, other: &'a f64) -> Float<P> {
        use FloatInner::*;

        let result = match self.as_f64() {
            Some(value) => (value * other).into(),
            None if other.is_one() => self.clone(),
            None if other.is_zero() => 0f64.into(),
            None => match &**self {
                Mul(ratio, args) => {
                    let mut ratio = ratio.clone();
                    ratio *= FloatRatio::new_constant(*other);
                    Mul(ratio, args.clone()).into()
                }
                Diff(ediff) => (ediff * other).into(),
                Max(emax) => (emax * other).into(),
                _ => Mul((*other).into(), vec![self.clone()]).into(),
            },
        };

        #[cfg(verify)]
        {
            if other > 0. {
                assert!(is_close(self.min_value() * other, result.min_value()));
                assert!(is_close(self.max_value() * other, result.max_value()));
            } else {
                assert!(is_close(self.min_value() * other, result.max_value()));
                assert!(is_close(self.max_value() * other, result.min_value()));
            }
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Mul<Output = Float<P>>, mul for Float<P>, f64);

impl<'a, 'b, P> Mul<&'a Float<P>> for &'b Float<P>
where
    P: Atom,
{
    type Output = Float<P>;

    fn mul(self, other: &'a Float<P>) -> Float<P> {
        use FloatInner::*;
        let result;

        trace!("mul lhs: {}", self);
        trace!("mul rhs: {}", other);

        if let Some(value) = other.as_f64() {
            result = ops::Mul::mul(self, value)
        } else if let Some(value) = self.as_f64() {
            result = ops::Mul::mul(other, value)
        } else {
            result = match (&**self, &**other) {
                (Mul(lhs_ratio, lhs_args), Mul(rhs_ratio, rhs_args)) => {
                    let mut args = lhs_args.clone();
                    args.extend(rhs_args.iter().cloned());
                    Mul(lhs_ratio * rhs_ratio, args).into()
                }
                (Mul(lhs_ratio, lhs_args), Diff(ediff)) => {
                    let factor = lhs_ratio.inner.factor;
                    let naked = Float::from(Mul(
                        FloatRatio::new(
                            1f64,
                            lhs_ratio.inner.numer.clone(),
                            lhs_ratio.inner.denom.clone(),
                        ),
                        lhs_args.clone(),
                    ));
                    let (constants, values): (Vec<_>, Vec<_>) =
                        if ediff.constant.is_zero() {
                            None
                        } else {
                            Some((factor * ediff.constant, naked.clone()))
                        }
                        .into_iter()
                        .chain(
                            ediff
                                .values
                                .iter()
                                .map(|(weight, item)| (weight * factor, item * &naked)),
                        )
                        .partition_map(|(weight, item)| {
                            use itertools::Either::*;

                            if item.is_one() {
                                Left(weight)
                            } else {
                                Right((weight, item))
                            }
                        });

                    DiffExpr {
                        constant: constants.into_iter().sum::<f64>(),
                        values: values.into_iter().collect(),
                    }
                    .into()
                }
                (Mul(ratio, args), Max(max)) | (Max(max), Mul(ratio, args))
                    if args.is_empty() =>
                {
                    (max * ratio).into()
                }
                (Mul(lhs_ratio, lhs_args), _) => {
                    let mut args = lhs_args.clone();
                    args.push(other.clone());
                    Mul(lhs_ratio.clone(), args).into()
                }
                (_, Mul(_, _)) => ops::Mul::mul(other, self),
                (_, _) => {
                    trace!("true mul: {:?} and {:?}", self, other);
                    Mul(1f64.into(), vec![self.clone(), other.clone()]).into()
                }
            }
        }

        trace!("mul out: {}", result);

        #[cfg(verify)]
        {
            assert!(self.min_value() > 0.);
            assert!(other.min_value() > 0.);

            assert!(is_close(
                self.min_value() * other.min_value(),
                result.min_value()
            ));
            assert!(is_close(
                self.max_value() * other.max_value(),
                result.max_value()
            ));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Mul<Output = Float<P>>, mul for Float<P>, Float<P>);

impl<P> MulAssign<&'_ Int<P>> for Float<P>
where
    P: Atom,
{
    fn mul_assign(&mut self, rhs: &'_ Int<P>) {
        MulAssign::mul_assign(self, rhs.to_symbolic_float());
    }
}

forward_binop_to_op_assign!(impl(P: Atom) Mul, mul for Float<P>, Int<P>, MulAssign, mul_assign);
forward_binop_to_ref_val_commutative!(impl(P: Atom) Mul<Output = Float<P>>, mul for Int<P>, Float<P>);

impl<'a, 'b, P> Add<&'a f64> for &'b Float<P>
where
    P: Atom,
{
    type Output = Float<P>;

    fn add(self, other: &'a f64) -> Float<P> {
        if let Some(value) = self.as_f64() {
            (value + other).into()
        } else if *other == 0. {
            self.clone()
        } else {
            match &**self {
                FloatInner::Max(max) => ops::Add::add(max, other).into(),
                _ => ops::Add::add(DiffExpr::from(self.clone()), other).into(),
            }
        }
    }
}

impl<P> AddAssign<&'_ Float<P>> for Float<P>
where
    P: Atom,
{
    fn add_assign(&mut self, rhs: &'_ Float<P>) {
        *self = Add::add(&self.clone(), rhs);
    }
}

impl<P> AddAssign<Float<P>> for Float<P>
where
    P: Atom,
{
    fn add_assign(&mut self, rhs: Float<P>) {
        *self = Add::add(&self.clone(), rhs);
    }
}

impl<'a, 'b, P> Add<&'a Float<P>> for &'b Float<P>
where
    P: Atom,
{
    type Output = Float<P>;

    fn add(self, other: &'a Float<P>) -> Float<P> {
        trace!("add lhs: {}", self);
        trace!("add rhs: {}", other);

        let result = if let Some(value) = other.as_f64() {
            ops::Add::add(self, &value)
        } else if let Some(value) = self.as_f64() {
            ops::Add::add(other, &value)
        } else {
            ops::Add::add(DiffExpr::from(self.clone()), DiffExpr::from(other.clone()))
                .into()
        };

        trace!("add out: {}", result);

        #[cfg(verify)]
        {
            assert!(is_close(
                self.min_value() + other.min_value(),
                result.min_value()
            ));
            assert!(is_close(
                self.max_value() + other.max_value(),
                result.max_value()
            ));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Add<Output = Float<P>>, add for Float<P>, Float<P>);

impl<'a, P> DivAssign<&'a Int<P>> for Float<P>
where
    P: Atom,
{
    fn div_assign(&mut self, rhs: &'a Int<P>) {
        trace!("div lhs: {}", self);
        trace!("div rhs: {}", rhs);

        match &*rhs.inner {
            IntInner::Mul(ratio) => {
                let mut flt_ratio = FloatRatioInner::from(ratio.clone());
                std::mem::swap(&mut flt_ratio.numer, &mut flt_ratio.denom);
                flt_ratio.factor = flt_ratio.factor.recip();
                *self *= Float::from(FloatInner::Mul(
                    FloatRatio {
                        inner: Rc::new(flt_ratio),
                    },
                    vec![],
                ));
            }
            _ => unimplemented!("{} / {}", self, rhs),
        }

        trace!("div out: {}", self);
    }
}

forward_binop_to_op_assign!(impl(P: Atom) Div, div for Float<P>, Int<P>, DivAssign, div_assign);

impl<'a, P> DivAssign<&'a f64> for Float<P>
where
    P: Atom,
{
    fn div_assign(&mut self, rhs: &'a f64) {
        self.mul_assign(&Float::from(rhs.recip()));
    }
}

forward_binop_to_op_assign!(impl(P: Atom) Div, div for Float<P>, f64, DivAssign, div_assign);

impl<P> SubAssign<&'_ f64> for Float<P>
where
    P: Atom,
{
    fn sub_assign(&mut self, other: &'_ f64) {
        *self = ops::Sub::sub(self.clone(), other);
    }
}

impl<P> SubAssign<f64> for Float<P>
where
    P: Atom,
{
    fn sub_assign(&mut self, other: f64) {
        *self = ops::Sub::sub(self.clone(), other);
    }
}

impl<'a, 'b, P> Sub<&'a f64> for &'b Float<P>
where
    P: Atom,
{
    type Output = Float<P>;

    fn sub(self, other: &'a f64) -> Float<P> {
        if let Some(value) = self.as_f64() {
            (value - other).into()
        } else if *other == 0. {
            self.clone()
        } else {
            ops::Sub::sub(DiffExpr::from(self.clone()), other).into()
        }
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Sub<Output = Float<P>>, sub for Float<P>, f64);

impl<'a, 'b, P> Sub<&'a Float<P>> for &'b Float<P>
where
    P: Atom,
{
    type Output = Float<P>;

    fn sub(self, other: &'a Float<P>) -> Float<P> {
        trace!("sub lhs: {}", self);
        trace!("sub rhs: {}", other);

        let result = if self == other {
            0f64.into()
        } else if let Some(value) = other.as_f64() {
            ops::Sub::sub(self, value)
        } else {
            ops::Sub::sub(DiffExpr::from(self.clone()), DiffExpr::from(other.clone()))
                .into()
        };

        trace!("sub out: {}", result);
        #[cfg(verify)]
        {
            assert!(is_close(
                self.min_value() - other.max_value(),
                result.min_value()
            ));
            assert!(is_close(
                self.max_value() - other.min_value(),
                result.max_value()
            ));
        }

        result
    }
}

forward_binop_to_ref_ref!(impl(P: Atom) Sub<Output = Float<P>>, sub for Float<P>, Float<P>);

impl<P> SubAssign<Float<P>> for Float<P>
where
    P: Atom,
{
    fn sub_assign(&mut self, rhs: Float<P>) {
        *self = Sub::sub(self.clone(), rhs);
    }
}

impl<P> SubAssign<&'_ Float<P>> for Float<P>
where
    P: Atom,
{
    fn sub_assign(&mut self, rhs: &'_ Float<P>) {
        *self = Sub::sub(self.clone(), rhs);
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::fmt;

    use num::{pow::pow, BigUint, Integer, One, ToPrimitive, Zero};
    use proptest::prelude::*;
    use proptest::sample::SizeRange;

    use super::{Factor, Range};

    macro_rules! assert_close {
        ($left:expr, $right:expr) => ({
            match (&$left, &$right) {
                (left_val, right_val) => {
                    if !$crate::is_close(*left_val, *right_val) {
                        // The reborrows below are intentional.  Without them, the stack slot for
                        // the borrow is initialized even before the values are compared, leading
                        // to a noticeable slow down.
                        panic!(r#"assertion failed: `is_close({}, {})`
  left: `{:?}`
 right: `{:?}`"#, stringify!($left), stringify!($right), *left_val, &*right_val)
                    }
                }
            }
        });
        ($left:expr, $right:expr,) => ({
            assert_close!($left, $right)
        });
        ($left:expr, $right:expr, $($arg:tt)+) => ({
            match (&$left, &$right) {
                (left_val, right_val) => {
                    if !$crate::is_close(*left_val, *right_val) {
                        // The reborrows below are intentional.  Without them, the stack slot for
                        // the borrow is initialized even before the values are compared, leading
                        // to a noticeable slow down.
                        panic!(r#"assertion failed: `is_close({}, {})`
  left: `{:?}`
 right: `{:?}`: {}"#, stringify!($left), stringify!($right), &*left_val, &*right_val, format_args!($($arg)+))
                    }
                }
            }
        });
    }

    macro_rules! assert_lt_close {
        ($left:expr, $right:expr) => ({
            match (&$left, &$right) {
                (left_val, right_val) => {
                    if !(*left_val < *right_val || $crate::is_close(*left_val, *right_val)) {
                        // The reborrows below are intentional.  Without them, the stack slot for
                        // the borrow is initialized even before the values are compared, leading
                        // to a noticeable slow down.
                        panic!(r#"assertion failed: `{} <~ {}`
  left: `{:?}`
 right: `{:?}`"#, stringify!($left), stringify!($right), *left_val, &*right_val)
                    }
                }
            }
        });
        ($left:expr, $right:expr,) => ({
            assert_lt_close!($left, $right)
        });
        ($left:expr, $right:expr, $($arg:tt)+) => ({
            match (&$left, &$right) {
                (left_val, right_val) => {
                    if !(*left_val < *right_val || $crate::is_close(*left_val, *right_val)) {
                        // The reborrows below are intentional.  Without them, the stack slot for
                        // the borrow is initialized even before the values are compared, leading
                        // to a noticeable slow down.
                        panic!(r#"assertion failed: `{} <~ {}`
  left: `{:?}`
 right: `{:?}`: {}"#, stringify!($left), stringify!($right), &*left_val, &*right_val, format_args!($($arg)+))
                    }
                }
            }
        });
    }

    macro_rules! assert_gt_close {
        ($left:expr, $right:expr) => ({
            match (&$left, &$right) {
                (left_val, right_val) => {
                    if !(*left_val > *right_val || $crate::is_close(*left_val, *right_val)) {
                        // The reborrows below are intentional.  Without them, the stack slot for
                        // the borrow is initialized even before the values are compared, leading
                        // to a noticeable slow down.
                        panic!(r#"assertion failed: `{} >~ {}`
  left: `{:?}`
 right: `{:?}`"#, stringify!($left), stringify!($right), *left_val, &*right_val)
                    }
                }
            }
        });
        ($left:expr, $right:expr,) => ({
            assert_gt_close!($left, $right)
        });
        ($left:expr, $right:expr, $($arg:tt)+) => ({
            match (&$left, &$right) {
                (left_val, right_val) => {
                    if !(*left_val > *right_val || $crate::is_close(*left_val, *right_val)) {
                        // The reborrows below are intentional.  Without them, the stack slot for
                        // the borrow is initialized even before the values are compared, leading
                        // to a noticeable slow down.
                        panic!(r#"assertion failed: `{} >~ {}`
  left: `{:?}`
 right: `{:?}`: {}"#, stringify!($left), stringify!($right), &*left_val, &*right_val, format_args!($($arg)+))
                    }
                }
            }
        });
    }

    #[derive(Clone, Debug, Hash)]
    struct Size<'a> {
        name: Cow<'a, str>,
        min: u64,
        max: u64,
    }

    fn arb_size_values(
        size: impl Into<SizeRange>,
    ) -> impl Strategy<Value = Vec<(Size<'static>, u64)>> {
        prop::sample::subsequence(
            (1..4usize)
                .flat_map(move |min| {
                    (min..(min + 4)).map(move |max| {
                        (
                            Size {
                                name: Cow::Owned(format!("x_{}_{}", min, max)),
                                min: pow(2, min),
                                max: pow(2, max),
                            },
                            (min..=max),
                        )
                    })
                })
                .collect::<Vec<_>>(),
            size,
        )
        .prop_flat_map(|seq| {
            let (sizes, ranges): (Vec<_>, Vec<_>) = seq.into_iter().unzip();

            ranges.prop_map(move |ranges| {
                sizes
                    .iter()
                    .cloned()
                    .zip(ranges.into_iter().map(|value| pow(2, value)))
                    .collect()
            })
        })
    }

    fn arb_ratio_inner<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::RatioInner<P>, BigUint)>
    where
        P: super::Atom,
    {
        (
            0..10usize,
            prop::collection::vec(any::<prop::sample::Index>(), 0..4),
            prop::collection::vec(any::<prop::sample::Index>(), 0..4),
        )
            .prop_map(move |(factor, numer, denom)| {
                let factor = pow(BigUint::from(2u32), factor);
                let (mut numer, mut numer_vals): (Vec<_>, Vec<_>) =
                    numer.into_iter().map(|x| x.get(&sizes).clone()).unzip();
                let (mut denom, mut denom_vals): (Vec<_>, Vec<_>) =
                    denom.into_iter().map(|x| x.get(&sizes).clone()).unzip();

                let mut to_remove = Vec::new();
                for (npos, n) in numer.iter().enumerate() {
                    if let Ok(dpos) = denom.binary_search(n) {
                        denom.remove(dpos);
                        denom_vals.remove(dpos);
                        to_remove.push(npos);
                    }
                }

                for npos in to_remove.into_iter().rev() {
                    numer.remove(npos);
                    numer_vals.remove(npos);
                }

                numer.sort();
                denom.sort();

                // Ensure the ratio is an integer
                let factor =
                    factor * denom.iter().map(Factor::lcm_value).product::<u64>();

                let value = &factor * numer_vals.iter().product::<u64>()
                    / denom_vals.iter().product::<u64>();

                (
                    super::RatioInner {
                        factor,
                        numer,
                        denom,
                    },
                    value,
                )
            })
    }

    fn arb_ratio<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::Ratio<P>, BigUint)>
    where
        P: super::Atom,
    {
        arb_ratio_inner(sizes)
            .prop_map(|(ratio_inner, val)| (super::Ratio::from(ratio_inner), val))
    }

    #[test]
    fn test_ratio_range() {
        use proptest::test_runner::{Config, TestRunner};

        let mut runner = TestRunner::new(Config::with_source_file(file!()));
        match runner.run(
            &arb_size_values(1..10).prop_flat_map(arb_ratio),
            |(ratio, value)| {
                use super::{Factor, Range};

                let min = BigUint::from(ratio.min_value());
                let max = BigUint::from(ratio.max_value());
                let gcd = BigUint::from(ratio.gcd_value());
                let lcm = BigUint::from(ratio.lcm_value());

                assert!(min <= value);
                assert!(value <= max);
                assert!(value.is_multiple_of(&gcd));
                assert!(lcm.is_multiple_of(&value));

                Ok(())
            },
        ) {
            Ok(()) => (),
            Err(e) => panic!("{}\n{}", e, runner),
        }
    }

    #[test]
    fn test_lcm() {
        use proptest::test_runner::{Config, TestRunner};

        let mut runner = TestRunner::new(Config::with_source_file(file!()));
        match runner.run(
            &arb_size_values(1..10)
                .prop_flat_map(|sizes| prop::collection::vec(arb_ratio(sizes), 2..10)),
            |ratios| {
                let (ratios, _): (Vec<_>, Vec<_>) = ratios.into_iter().unzip();
                let lcm = super::LcmExpr::new(ratios.iter().cloned())
                    .unwrap_or_else(super::LcmExpr::one);

                for ratio in ratios {
                    assert!(lcm.gcd.is_multiple_of(&BigUint::from(ratio.gcd_value())));
                    assert!(lcm.lcm.is_multiple_of(&BigUint::from(ratio.lcm_value())));
                    assert!(
                        lcm.gcd.is_multiple_of(&BigUint::from(ratio.lcm_value()))
                            || lcm.args.iter().any(|elem| elem.is_multiple_of(&ratio)),
                        "{}[|{}, {}|] not a multiple of {}[|{}, {}|]",
                        lcm,
                        lcm.gcd,
                        lcm.lcm,
                        ratio,
                        ratio.gcd_value(),
                        ratio.lcm_value(),
                    );
                }

                Ok(())
            },
        ) {
            Ok(()) => (),
            Err(e) => panic!("{}\n{}", e, runner),
        }
    }

    fn arb_lcm<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::LcmExpr<P>, BigUint)>
    where
        P: super::Atom,
    {
        prop::collection::vec(arb_ratio(sizes), 2..10).prop_map(|ratios| {
            let (ratios, values): (Vec<_>, Vec<_>) = ratios.into_iter().unzip();

            let mut lcm_value = BigUint::one();
            for (ix, value) in values.into_iter().enumerate() {
                assert!(
                    value > BigUint::zero(),
                    "lcm: {} <= 0 (from {})",
                    value,
                    ratios[ix]
                );

                lcm_value = lcm_value.lcm(&value);
            }

            let lcm = super::LcmExpr::new(ratios.iter().cloned())
                .unwrap_or_else(super::LcmExpr::one);

            assert!(lcm_value > BigUint::zero(), "{} <= 0", lcm_value);

            (lcm, lcm_value)
        })
    }

    #[test]
    fn test_lcm_range() {
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();
        runner
            .run(
                &arb_size_values(1..10).prop_flat_map(arb_lcm),
                |(lcm, lcm_value)| {
                    assert!(lcm.gcd <= lcm_value);
                    assert!(lcm_value <= lcm.lcm);
                    assert!(lcm_value.is_multiple_of(&lcm.gcd));
                    assert!(lcm.lcm.is_multiple_of(&lcm_value));

                    Ok(())
                },
            )
            .unwrap();
    }

    #[test]
    fn test_min() {
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();
        runner
            .run(
                &arb_size_values(1..10).prop_flat_map(|sizes| {
                    prop::collection::vec(arb_ratio(sizes), 2..10)
                }),
                |ratios| {
                    let (ratios, _): (Vec<_>, Vec<_>) = ratios.into_iter().unzip();
                    let min = super::MinExpr::new(ratios.iter().cloned())
                        .unwrap_or_else(super::MinExpr::one);
                    assert!(min.min <= min.max);
                    assert!(ratios
                        .iter()
                        .any(|ratio| BigUint::from(ratio.min_value()) == min.min));

                    for ratio in ratios {
                        assert!(min.min <= BigUint::from(ratio.min_value()));
                        assert!(min.max <= BigUint::from(ratio.max_value()));
                        assert!(
                            min.max <= BigUint::from(ratio.min_value())
                                || min
                                    .values
                                    .iter()
                                    .any(|elem| elem.is_less_than(&ratio)),
                            "{} is not less than {}",
                            min,
                            ratio
                        );
                    }

                    Ok(())
                },
            )
            .unwrap();
    }

    fn arb_min<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::MinExpr<P>, BigUint)>
    where
        P: super::Atom,
    {
        prop::collection::vec(arb_ratio(sizes), 2..10).prop_map(|ratios| {
            let (ratios, values): (Vec<_>, Vec<_>) = ratios.into_iter().unzip();

            let min_value = values
                .into_iter()
                .enumerate()
                .map(|(ix, value)| {
                    assert!(
                        value > BigUint::zero(),
                        "min: {} <= 0 (from {})",
                        value,
                        ratios[ix]
                    );

                    value
                })
                .min()
                .expect("min of empty set");

            let min = super::MinExpr::new(ratios.iter().cloned())
                .unwrap_or_else(super::MinExpr::one);

            assert!(
                min_value > BigUint::zero(),
                "min({}): {} <= 0",
                min,
                min_value
            );

            (min, min_value)
        })
    }

    #[test]
    fn test_min_range() {
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();
        runner
            .run(
                &arb_size_values(1..10).prop_flat_map(arb_min),
                |(min, min_value)| {
                    assert!(
                        min.min <= min_value,
                        "{}: min should be {} but got {}",
                        min,
                        min.min,
                        min_value
                    );
                    assert!(
                        min_value <= min.max,
                        "{}: max should be {} but got {}",
                        min,
                        min.max,
                        min_value,
                    );

                    Ok(())
                },
            )
            .unwrap();
    }

    fn arb_int_inner<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::IntInner<P>, BigUint)>
    where
        P: super::Atom,
    {
        prop_oneof![
            arb_lcm(sizes.clone())
                .prop_map(|(lcm, val)| (super::IntInner::Lcm(lcm), val)),
            arb_min(sizes.clone())
                .prop_map(|(min, val)| (super::IntInner::Min(min), val)),
            arb_ratio(sizes).prop_map(|(ratio, val)| (super::IntInner::Mul(ratio), val)),
        ]
    }

    fn arb_int<P>(sizes: Vec<(P, u64)>) -> impl Strategy<Value = (super::Int<P>, BigUint)>
    where
        P: super::Atom,
    {
        arb_int_inner(sizes).prop_map(|(inner, val)| (inner.into(), val))
    }

    #[test]
    fn test_int_mul_num() {
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();
        runner
            .run(
                &(arb_size_values(1..10).prop_flat_map(arb_int), 1..256u64),
                |((a, av), bv)| {
                    let prod = &a * &BigUint::from(bv);

                    assert!(
                        a.min_value() * bv == prod.min_value(),
                        "wrong min_value for {} * {} (= {}): {} vs {}",
                        a,
                        bv,
                        prod,
                        a.min_value() * bv,
                        prod.min_value()
                    );

                    assert!(
                        a.max_value() * bv == prod.max_value(),
                        "wrong max_value for {} * {} (= {}): {} vs {}",
                        a,
                        bv,
                        prod,
                        a.max_value() * bv,
                        prod.max_value()
                    );

                    assert!(BigUint::from(prod.min_value()) <= &av * bv);
                    assert!(&av * bv <= BigUint::from(prod.max_value()));

                    Ok(())
                },
            )
            .unwrap();
    }

    #[test]
    fn test_int_mul_ratio() {
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();
        runner
            .run(
                &arb_size_values(1..10)
                    .prop_flat_map(|sizes| (arb_int(sizes.clone()), arb_ratio(sizes))),
                |((a, av), (b, bv))| {
                    let prod = &a * &b;

                    // Simplifications can occur which make the min_value increase
                    assert!(
                        a.min_value() * b.min_value() <= prod.min_value(),
                        "wrong min_value for {} * {} (= {}): {} vs {}",
                        a,
                        b,
                        prod,
                        a.min_value() * b.min_value(),
                        prod.min_value()
                    );

                    // Simplifications can occur which make the max_value decrease
                    assert!(
                        a.max_value() * b.max_value() >= prod.max_value(),
                        "wrong max_value for {} * {} (= {}): {} vs {}",
                        a,
                        b,
                        prod,
                        a.max_value() * b.max_value(),
                        prod.max_value()
                    );

                    assert!(
                        BigUint::from(prod.min_value()) <= &av * &bv,
                        "wrong min_value for {} * {} (= {})",
                        a,
                        b,
                        prod
                    );
                    assert!(
                        BigUint::from(prod.max_value()) >= &av * &bv,
                        "wrong max_value for {} * {} (= {})",
                        a,
                        b,
                        prod
                    );

                    Ok(())
                },
            )
            .unwrap();
    }

    #[test]
    fn test_to_symbolic_float() {
        use proptest::test_runner::TestRunner;

        let mut runner = TestRunner::default();
        runner
            .run(
                &arb_size_values(1..10).prop_flat_map(arb_int).prop_filter(
                    "lcm can't be converted to float",
                    |(int, _)| match &*int.inner {
                        super::IntInner::Lcm(_) => false,
                        _ => true,
                    },
                ),
                |(int, val)| {
                    let flt = int.to_symbolic_float();

                    assert!(
                        super::is_close(flt.min_value(), int.min_value() as f64),
                        "(float){} = {}; min was {} but is now {}",
                        int,
                        flt,
                        int.min_value(),
                        flt.min_value()
                    );
                    assert!(
                        super::is_close(flt.max_value(), int.max_value() as f64),
                        "(float){} = {}; max was {} but is now {}",
                        int,
                        flt,
                        int.max_value(),
                        flt.max_value()
                    );

                    let flt_val = val.to_u64().unwrap() as f64;
                    assert!(flt.min_value() <= flt_val);
                    assert!(flt_val <= flt.max_value());

                    Ok(())
                },
            )
            .unwrap();
    }

    fn arb_float_ratio_inner<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::FloatRatioInner<P>, f64)>
    where
        P: super::Atom,
    {
        arb_ratio(sizes)
            .prop_map(|(ratio, val)| (ratio.into(), val.to_u64().unwrap() as f64))
    }

    fn arb_float_ratio<P>(
        sizes: Vec<(P, u64)>,
    ) -> impl Strategy<Value = (super::FloatRatio<P>, f64)>
    where
        P: super::Atom,
    {
        arb_float_ratio_inner(sizes).prop_map(|(fratio, val)| (fratio.into(), val))
    }

    fn arb_float<P>(sizes: Vec<(P, u64)>) -> impl Strategy<Value = (super::Float<P>, f64)>
    where
        P: super::Atom + 'static,
    {
        prop_oneof![
            arb_float_ratio(sizes.clone()).prop_map(|(fratio, val)| (
                super::FloatInner::Mul(fratio, Vec::new()).into(),
                val
            )),
            (arb_ratio(sizes.clone()), 2..=8u32).prop_map(|((ratio, val), denom)| (
                super::Float::div_ceil(&ratio.clone().into(), denom),
                ((&val + denom - 1u32) / denom).to_u64().unwrap() as f64
            )),
            (1..10u32)
                .prop_map(f64::from)
                .prop_map(|val| (val.into(), val)),
        ]
        .prop_recursive(
            8,   // levels deep
            256, // maximum size
            10,  // up to 10 items
            move |inner| {
                prop_oneof![
                    (
                        arb_float_ratio(sizes.clone()),
                        prop::collection::vec(inner.clone(), 1..=1)
                    )
                        .prop_map(|((fratio, rval), args)| {
                            let (args, avals): (Vec<_>, Vec<_>) =
                                args.into_iter().unzip();
                            (
                                super::FloatInner::Mul(fratio, args).into(),
                                rval * avals.into_iter().product::<f64>(),
                            )
                        }),
                    (
                        1..200u32,
                        prop::collection::vec((1..100u32, inner.clone()), 1..10)
                    )
                        .prop_map(|(constant, args)| {
                            let constant = (f64::from(constant) - 100.5) / 100.;
                            let (args, vals): (super::WeightedVecSet<_>, Vec<_>) = args
                                .into_iter()
                                .map(|(weight, (flt, val))| {
                                    let weight = f64::from(weight);
                                    ((weight, flt), (weight * val))
                                })
                                .unzip();
                            let minv = args
                                .iter()
                                .map(|(weight, flt)| weight * flt.min_value())
                                .sum::<f64>();
                            let constant = if minv < 0. && constant + minv < 0. {
                                constant - minv
                            } else {
                                constant
                            };
                            (
                                {
                                    super::FloatInner::Diff(super::DiffExpr {
                                        constant,
                                        values: args,
                                    })
                                    .into()
                                },
                                constant + vals.into_iter().sum::<f64>(),
                            )
                        }),
                    prop::collection::vec(inner.clone(), 1..10).prop_map(|args| {
                        let (args, vals): (Vec<_>, Vec<_>) = args
                            .into_iter()
                            .map(|(arg, val)| (super::DiffExpr::from(arg), val))
                            .unzip();

                        let min = args
                            .iter()
                            .map(|arg| arg.min_value())
                            .fold(std::f64::NEG_INFINITY, f64::max);
                        let max = args
                            .iter()
                            .map(|arg| arg.max_value())
                            .fold(std::f64::NEG_INFINITY, f64::max);

                        (
                            super::FloatInner::Max(super::FMaxExpr::new(
                                min,
                                max,
                                args.into_iter().collect(),
                            ))
                            .into(),
                            vals.into_iter().fold(std::f64::NEG_INFINITY, f64::max),
                        )
                    }),
                    prop::collection::vec(inner.clone(), 1..10).prop_map(|args| {
                        let (args, vals): (Vec<_>, Vec<_>) = args
                            .into_iter()
                            .map(|(arg, val)| (super::DiffExpr::from(arg), val))
                            .unzip();

                        let min = args
                            .iter()
                            .map(|arg| arg.min_value())
                            .fold(std::f64::INFINITY, f64::min);
                        let max = args
                            .iter()
                            .map(|arg| arg.max_value())
                            .fold(std::f64::INFINITY, f64::min);

                        (
                            super::FloatInner::Min(super::FMinExpr {
                                min,
                                max,
                                values: args.into_iter().collect(),
                            })
                            .into(),
                            vals.into_iter().fold(std::f64::INFINITY, f64::min),
                        )
                    }),
                ]
            },
        )
    }

    #[test]
    fn test_flt_add_num() {
        use proptest::test_runner::{TestError, TestRunner};

        let mut runner = TestRunner::default();
        let result =  runner
            .run(
                &(
                    arb_size_values(1..10).prop_flat_map(arb_float),
                    1..20u32,
                ),
                |((a, av), bv)| {
                    let bv = f64::from(bv);
                    let sum = &a + &bv;

                    assert!(
                        super::is_close(a.min_value() + bv, sum.min_value()),
                        "wrong min_value for {} + {} (= {}): {} vs {}",
                        a,
                        bv,
                        sum,
                        a.min_value() + bv,
                        sum.min_value()
                    );

                    assert!(
                        super::is_close(a.max_value() + bv, sum.max_value()),
                        "wrong max_value for {} + {} (= {}): {} vs {}",
                        a,
                        bv,
                        sum,
                        a.max_value() + bv,
                        sum.max_value()
                    );

                    assert!(
                        sum.min_value() <= av + bv,
                        "assertion failed: ({} + {:e} = {}).min_value() <= {} + {:e} ({:e} > {:e})",
                        a,
                        bv,
                        &sum,
                        av,
                        bv,
                        sum.min_value(),
                        av + bv,
                    );
                    assert!(
                        av + bv <= sum.max_value(),
                        "assertion failed: {:e} + {:e} <= ({} + {:e} = {}).max_value() ({:e} > {:e})",
                        av,
                        bv,
                        a,
                        bv,
                        &sum,
                        av + bv,
                        sum.max_value(),
                    );

                    Ok(())
                },
            );

        match result {
            Ok(()) => (),
            Err(TestError::Fail(_, ((a, av), bv))) => {
                panic!(
                    "Found minimal failing case: 
a = {a} [{a:?}]
av = {av} [{av:?}]
bv = {bv} [{bv:?}]",
                    a = a,
                    av = av,
                    bv = bv
                );
            }
            Err(err) => panic!("Unexpected error: {:?}", err),
        }
    }

    #[test]
    fn test_flt_add_flt() {
        use proptest::test_runner::{TestError, TestRunner};

        let mut runner = TestRunner::default();
        let result = runner.run(
            &arb_size_values(1..10)
                .prop_flat_map(|sizes| (arb_float(sizes.clone()), arb_float(sizes))),
            |((a, av), (b, bv))| {
                let sum = &a + &b;

                assert_close!(sum.min_value(), a.min_value() + b.min_value());

                assert_close!(sum.max_value(), a.max_value() + b.max_value());

                assert_lt_close!(sum.min_value(), av + bv,);
                assert_gt_close!(sum.max_value(), av + bv);

                Ok(())
            },
        );

        match result {
            Ok(()) => (),
            Err(TestError::Fail(_, ((a, av), (b, bv)))) => {
                panic!(
                    "Found minimal failing case: 
a = {a} [{a:?}]
av = {av} [{av:?}]
b = {b} [{b:?}]
bv = {bv} [{bv:?}]",
                    a = a,
                    av = av,
                    b = b,
                    bv = bv
                );
            }
            Err(err) => panic!("Unexpected error: {:?}", err),
        }
    }

    #[test]
    fn test_flt_mul_flt() {
        use proptest::test_runner::{TestError, TestRunner};

        let mut runner = TestRunner::default();
        let result = runner.run(
            &arb_size_values(1..10)
                .prop_flat_map(|sizes| (arb_float(sizes.clone()), arb_float(sizes))),
            |((a, av), (b, bv))| {
                let prod = &a * &b;

                // Simplifications can occur which make the min_value increase
                assert_gt_close!(
                    prod.min_value(),
                    a.min_value() * b.min_value(),
                    "product of min is {:e} but min of prod is {:e}",
                    a.min_value() * b.min_value(),
                    prod.min_value(),
                );

                // Simplifications can occur which make the max_value decrease
                assert_lt_close!(
                    prod.max_value(),
                    a.max_value() * b.max_value(),
                    "product of max is {:e} but max of prod is {:e}",
                    a.max_value() * b.max_value(),
                    prod.max_value()
                );

                assert_lt_close!(prod.min_value(), av * bv);
                assert_gt_close!(prod.max_value(), av * bv);

                Ok(())
            },
        );

        match result {
            Ok(()) => (),
            Err(TestError::Fail(_, ((a, av), (b, bv)))) => {
                panic!(
                    "Found minimal failing case: 
a = {a} [{a:?}]
av = {av}
b = {b} [{b:?}]
bv = {bv}
prod = {prod}",
                    a = &a,
                    av = av,
                    b = &b,
                    bv = bv,
                    prod = &a * &b,
                );
            }
            Err(err) => panic!("Unexpected error: {:?}", err),
        }
    }

    #[test]
    fn test_div_ceil() {
        use proptest::test_runner::{Config, TestRunner};

        let mut runner = TestRunner::new(Config::with_source_file(file!()));
        match runner.run(
            &(arb_size_values(1..10).prop_flat_map(arb_int), 1..10u32),
            |((a, av), denom)| {
                let divceil = super::Float::div_ceil(&a, denom);

                let denom = u64::from(denom);
                assert_close!(
                    ((a.min_value() + denom - 1) / denom) as f64,
                    divceil.min_value(),
                );

                assert_close!(
                    ((a.max_value() + denom - 1) / denom) as f64,
                    divceil.max_value(),
                );

                assert_lt_close!(
                    divceil.min_value(),
                    ((&av + denom - 1u32) / denom).to_u64().unwrap() as f64
                );

                assert_gt_close!(
                    divceil.max_value(),
                    ((&av + denom - 1u32) / denom).to_u64().unwrap() as f64
                );

                Ok(())
            },
        ) {
            Ok(()) => (),
            Err(e) => panic!("{}\n{}", e, runner),
        }
    }

    #[test]
    fn test_flt_max_flt() {
        use proptest::test_runner::{Config, TestRunner};

        let mut runner = TestRunner::new(Config::with_source_file(file!()));
        match runner.run(
            &arb_size_values(1..10)
                .prop_flat_map(|sizes| (arb_float(sizes.clone()), arb_float(sizes))),
            |((a, av), (b, bv))| {
                let max = super::Float::max(&a, &b);

                assert!(
                    super::is_close(a.min_value(), max.min_value())
                        || super::is_close(b.min_value(), max.min_value())
                );

                assert!(
                    super::is_close(a.max_value(), max.max_value())
                        || super::is_close(b.max_value(), max.max_value())
                );

                assert_close!(a.min_value().max(b.min_value()), max.min_value());

                assert_close!(
                    a.max_value().max(b.max_value()),
                    max.max_value(),
                    "
 max: `{}`,
   a: `{}` [{} <= {}],
   b: `{}` [{} <= {}]",
                    max,
                    a,
                    a.min_value(),
                    a.max_value(),
                    b,
                    b.min_value(),
                    b.max_value(),
                );

                assert_lt_close!(
                    max.min_value(),
                    av.max(bv),
                    "
 max: `{}`,
  av: `{}`,
  bv: `{}`",
                    max,
                    av,
                    bv
                );
                assert_lt_close!(av.max(bv), max.max_value());

                Ok(())
            },
        ) {
            Ok(()) => (),
            Err(e) => panic!("{}\n{}", e, runner),
        }
    }

    #[test]
    fn test_flt_min_flt() {
        use proptest::test_runner::{Config, TestRunner};

        let mut runner = TestRunner::new(Config::with_source_file(file!()));
        match runner.run(
            &arb_size_values(1..10)
                .prop_flat_map(|sizes| (arb_float(sizes.clone()), arb_float(sizes))),
            |((a, av), (b, bv))| {
                let min = super::Float::min(&a, &b);

                assert_close!(
                    a.min_value().min(b.min_value()),
                    min.min_value(),
                    "
   a: `{a}`
   b: `{b}`
 min: `{min}`",
                    a = a,
                    b = b,
                    min = min
                );
                assert_close!(
                    a.max_value().min(b.max_value()),
                    min.max_value(),
                    "
   a: `{a}`
   b: `{b}`
 min: `{min}`",
                    a = a,
                    b = b,
                    min = min
                );

                assert_lt_close!(min.min_value(), av.min(bv));
                assert_lt_close!(av.min(bv), min.max_value());

                Ok(())
            },
        ) {
            Ok(()) => (),
            Err(e) => panic!("{}\n{}", e, runner),
        }
    }

    impl fmt::Display for Size<'_> {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            fmt::Display::fmt(&self.name, fmt)
        }
    }

    impl PartialEq for Size<'_> {
        fn eq(&self, other: &Size<'_>) -> bool {
            self.name == other.name
        }
    }

    impl Eq for Size<'_> {}

    impl Ord for Size<'_> {
        fn cmp(&self, other: &Size<'_>) -> std::cmp::Ordering {
            self.name.cmp(&other.name)
        }
    }

    impl PartialOrd for Size<'_> {
        fn partial_cmp(&self, other: &Size<'_>) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Range for Size<'_> {
        fn min_value(&self) -> u64 {
            self.min
        }

        fn max_value(&self) -> u64 {
            self.max
        }
    }

    impl Factor for Size<'_> {
        fn gcd_value(&self) -> u64 {
            self.min
        }

        fn lcm_value(&self) -> u64 {
            self.max
        }
    }

    impl<'a> Size<'a> {
        fn new<N>(name: N, min: u64, max: u64) -> Self
        where
            N: Into<Cow<'a, str>>,
        {
            Size {
                name: name.into(),
                min,
                max,
            }
        }
    }

    fn make<'a, N>(
        name: N,
        min: u64,
        max: u64,
    ) -> (super::Int<Size<'a>>, super::Int<Size<'a>>)
    where
        N: Into<Cow<'a, str>>,
    {
        let name = name.into();
        let size = Size::new(name.clone(), min, max);
        (
            super::Int::ratio(1u32, vec![size.clone()], vec![]),
            super::Int::ratio(1u32, vec![], vec![size]),
        )
    }

    fn symbolic(value: u64) -> super::Int<Size<'static>> {
        super::Int::ratio(
            1u32,
            vec![Size::new(format!("_{}_", value), value, value)],
            vec![],
        )
    }

    #[test]
    fn prout() {
        let (x, _invx) = make("%4", 2, 4);

        let a = x.to_symbolic_float() * 445.945945945946
            + super::Float::from(93.62934362934362);
        let b = x.to_symbolic_float() * 484.5559845559846
            + super::Float::from(16.409266409266394);
        let c = x.to_symbolic_float() * 484.5559845559846
            + super::Float::from(16.409266409266408);

        eprintln!("diff: {}", (&a - &b).max_value());
        assert_lt_close!((&a - &b).max_value(), 0.);

        let amax = super::FMaxExpr::from(a.clone());
        let bmax = super::FMaxExpr::from(b.clone());
        assert!(amax.is_single_value() && bmax.is_single_value());

        let mut expr = a.clone();
        expr.max_assign(&b);
        // expr.max_assign(&c);

        assert_lt_close!(expr.max_value(), -1., "{}", expr);
    }

    #[test]
    fn it_works() {
        let (x, _invx) = make("x", 1, 10);

        // 1 + max(3, x) -> max(4, 1 + x)
        let mut expr = x.to_symbolic_float();
        expr.max_assign(&3f64.into());
        let expr = expr + super::Float::from(1f64);

        assert_eq!(format!("{}", expr), "max(4, 1 + x)");
    }

    #[test]
    fn still_works() {
        let (x, _invx) = make("x", 1, 10);
        let (y, _invy) = make("y", 1, 10);

        // y + max(3, x) -> max(4, y + 3, y + x)
        let mut expr = x.to_symbolic_float();
        expr.max_assign(&3f64.into());
        let expr = expr + y.to_symbolic_float();

        assert_eq!(format!("{}", expr), "max(4, x + y, 3 + y)");
    }

    #[test]
    fn not_broken() {
        let (x, _invx) = make("x", 1, 10);
        let (y, _invy) = make("y", 1, 10);

        // (9 + y) + max(3, x) -> max(13, y + 12, 9 + y + x)
        let mut expr = x.to_symbolic_float();
        expr.max_assign(&3f64.into());
        let expr = expr + (y.to_symbolic_float() + super::Float::from(9f64));

        assert_eq!(format!("{}", expr), "max(13, 9 + y + x, 12 + y)");
    }

    #[test]
    fn maximax() {
        let (x, _invx) = make("x", 1, 10);
        let (y, _invy) = make("y", 1, 10);

        // max(9, y) + max(3, x) -> max(13, y + 12, 9 + y + x)
        let mut expr = x.to_symbolic_float();
        expr.max_assign(&3f64.into());
        let mut ymax = y.to_symbolic_float();
        ymax.max_assign(&9f64.into());
        let expr = expr + ymax;

        assert_eq!(format!("{}", expr), "max(3, x) + max(9, y)");
    }

    #[test]
    fn max_dedup() {
        let (x, _) = make("x", 1, 10);

        let mut expr = x.to_symbolic_float();
        expr.max_assign(&x.to_symbolic_float());

        assert_eq!(format!("{}", expr), "x");
    }

    #[test]
    fn max_dedup_2() {
        let (x, _) = make("x", 3, 3);

        let mut expr = x.to_symbolic_float();
        expr.max_assign(&symbolic(2).to_symbolic_float());

        println!("{:?}", expr);

        assert_eq!(format!("{}", expr), "3");
    }

    #[test]
    fn max_lin() {
        let (x, _) = make("x", 1, 10);

        let mut expr = x.to_symbolic_float() * 2f64;
        expr.max_assign(&(x.to_symbolic_float() * 3f64));

        assert_eq!(format!("{}", expr), "3*x");
    }

    /// This test ensures that the lcm between two unrelatable expressions actually keeps the two
    /// expressions.
    #[test]
    fn lcm_len() {
        let x12 = Size::new("x_1_2", 2, 4);
        let x14 = Size::new("x_1_4", 2, 16);
        let x24 = Size::new("x_2_4", 4, 16);

        let a = super::Ratio::new(
            256u32.into(),
            vec![x12.clone()],
            vec![x14.clone(), x24.clone()],
        );
        let b = super::Ratio::new(4u32.into(), vec![x14.clone(), x14.clone()], vec![]);

        let lcm = super::LcmExpr::new(vec![a.clone(), b.clone()]).unwrap();

        assert!(lcm.args.len() == 2);
    }

    /// This test ensures that the minimum between two unrelatable expressions actually keeps the two
    /// expressions.
    #[test]
    fn min_len_regression() {
        let x12 = Size::new("x_1_2", 2, 4);
        let x14 = Size::new("x_1_4", 2, 16);
        let x24 = Size::new("x_2_4", 4, 16);

        let a = super::Ratio::new(
            256u32.into(),
            vec![x12.clone()],
            vec![x14.clone(), x24.clone()],
        );
        let b = super::Ratio::new(4u32.into(), vec![x14.clone(), x14.clone()], vec![]);

        let min = super::MinExpr::new(vec![a.clone(), b.clone()]).unwrap();

        assert!(min.values.len() == 2);
    }

    // TODO: test that `(a + b) * c => a * c + b * c
    // need to check with "args" in c, eg. 3 * div_ceil(...)
    //
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct VecSet<T>
where
    T: Hash,
{
    values: Rc<hash::MemoizedHash<Vec<T>>>,
}

impl<T> PartialOrd for VecSet<T>
where
    T: PartialOrd + Hash,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if Rc::ptr_eq(&self.values, &other.values) {
            Some(Ordering::Equal)
        } else {
            self.values.partial_cmp(&other.values)
        }
    }
}

impl<T> Ord for VecSet<T>
where
    T: Ord + Hash,
{
    fn cmp(&self, other: &Self) -> Ordering {
        if Rc::ptr_eq(&self.values, &other.values) {
            Ordering::Equal
        } else {
            self.values.cmp(&other.values)
        }
    }
}

impl<T> Default for VecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn default() -> Self {
        iter::empty().collect()
    }
}

impl<T> iter::FromIterator<T> for VecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn from_iter<II>(iter: II) -> Self
    where
        II: IntoIterator<Item = T>,
    {
        let mut values = Vec::from_iter(iter);
        values.sort();
        values.dedup();

        VecSet {
            values: Rc::new(hash::MemoizedHash::new(values)),
        }
    }
}

impl<T> VecSet<T>
where
    T: Hash,
{
    fn len(&self) -> usize {
        self.values.len()
    }

    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn iter(&self) -> std::slice::Iter<'_, T> {
        self.values.iter()
    }

    fn iter_mut(&mut self) -> std::slice::IterMut<'_, T>
    where
        T: Clone,
    {
        hash::MemoizedHash::make_mut(Rc::make_mut(&mut self.values)).iter_mut()
    }

    fn fast_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.values, &other.values)
    }
}

impl<T, I> ops::Index<I> for VecSet<T>
where
    T: Hash,
    Vec<T>: ops::Index<I>,
{
    type Output = <Vec<T> as ops::Index<I>>::Output;

    fn index(&self, index: I) -> &Self::Output {
        ops::Index::index(self.values.as_ref().as_ref(), index)
    }
}

struct Union<L, R>
where
    L: Iterator,
    R: Iterator,
{
    left: std::iter::Peekable<L>,
    right: std::iter::Peekable<R>,
}

impl<T, L, R> Union<L, R>
where
    T: Ord,
    L: Iterator<Item = T>,
    R: Iterator<Item = T>,
{
    fn new(left: L, right: R) -> Self {
        Union {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<T, L, R> Iterator for Union<L, R>
where
    T: Ord,
    L: Iterator<Item = T>,
    R: Iterator<Item = T>,
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if let Some(litem) = self.left.peek() {
            if let Some(ritem) = self.right.peek() {
                match litem.cmp(ritem) {
                    Ordering::Less => {
                        Some(self.left.next().unwrap_or_else(|| unreachable!()))
                    }
                    Ordering::Greater => {
                        Some(self.right.next().unwrap_or_else(|| unreachable!()))
                    }
                    Ordering::Equal => {
                        self.left.next().unwrap_or_else(|| unreachable!());
                        Some(self.right.next().unwrap_or_else(|| unreachable!()))
                    }
                }
            } else {
                Some(self.left.next().unwrap_or_else(|| unreachable!()))
            }
        } else {
            self.right.next()
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (llow, lhigh) = self.left.size_hint();
        let (rlow, rhigh) = self.right.size_hint();

        (
            llow.min(rlow),
            lhigh.and_then(|lhigh| rhigh.map(|rhigh| lhigh + rhigh)),
        )
    }
}

impl<T> VecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn new(value: T) -> Self {
        VecSet {
            values: Rc::new(hash::MemoizedHash::new(vec![value])),
        }
    }

    fn from_sorted_iter<II>(values: II) -> Self
    where
        II: IntoIterator<Item = T>,
    {
        VecSet {
            values: Rc::new(hash::MemoizedHash::new(values.into_iter().collect())),
        }
    }
}

struct WeightedUnion<L, R>
where
    L: Iterator,
    R: Iterator,
{
    left: std::iter::Peekable<L>,
    right: std::iter::Peekable<R>,
}

impl<T, L, R> WeightedUnion<L, R>
where
    T: Ord,
    L: Iterator<Item = (f64, T)>,
    R: Iterator<Item = (f64, T)>,
{
    fn new(left: L, right: R) -> Self {
        WeightedUnion {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<T, L, R> Iterator for WeightedUnion<L, R>
where
    T: Ord,
    L: Iterator<Item = (f64, T)>,
    R: Iterator<Item = (f64, T)>,
{
    type Item = (f64, T);

    fn next(&mut self) -> Option<(f64, T)> {
        loop {
            if let Some((_, litem)) = self.left.peek() {
                if let Some((_, ritem)) = self.right.peek() {
                    match litem.cmp(ritem) {
                        Ordering::Less => {
                            break Some(
                                self.left.next().unwrap_or_else(|| unreachable!()),
                            )
                        }
                        Ordering::Greater => {
                            break Some(
                                self.right.next().unwrap_or_else(|| unreachable!()),
                            )
                        }
                        Ordering::Equal => {
                            let (lweight, litem) =
                                self.left.next().unwrap_or_else(|| unreachable!());
                            let (rweight, _ritem) =
                                self.right.next().unwrap_or_else(|| unreachable!());

                            let weight = lweight + rweight;

                            if is_close(weight, 0.) {
                                continue;
                            } else {
                                break Some((weight, litem));
                            }
                        }
                    }
                } else {
                    break Some(self.left.next().unwrap_or_else(|| unreachable!()));
                }
            } else {
                break self.right.next();
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (llow, lhigh) = self.left.size_hint();
        let (rlow, rhigh) = self.right.size_hint();

        (
            llow.min(rlow),
            lhigh.and_then(|lhigh| rhigh.map(|rhigh| lhigh.max(rhigh))),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
struct WeightedVecSet<T>
where
    T: Hash,
{
    coefficients: VecSet<T>,
    weights: Rc<Vec<f64>>,
}

impl<T> Default for WeightedVecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn default() -> Self {
        WeightedVecSet {
            coefficients: VecSet::default(),
            weights: Rc::default(),
        }
    }
}

impl<T> Eq for WeightedVecSet<T> where T: Eq + Hash {}

impl<T> PartialOrd for WeightedVecSet<T>
where
    T: Ord + Hash,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for WeightedVecSet<T>
where
    T: Ord + Hash,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.coefficients.cmp(&other.coefficients).then_with(|| {
            for (lhs, rhs) in self.weights.iter().zip(other.weights.iter()) {
                if lhs < rhs {
                    return Ordering::Less;
                } else if lhs > rhs {
                    return Ordering::Greater;
                }
            }
            Ordering::Equal
        })
    }
}

impl<T> Hash for WeightedVecSet<T>
where
    T: Hash,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        for (factor, item) in self.iter() {
            factor.to_bits().hash(state);
            item.hash(state)
        }
    }
}

impl<T> WeightedVecSet<T>
where
    T: Hash,
{
    fn len(&self) -> usize {
        self.coefficients.len()
    }

    fn is_empty(&self) -> bool {
        self.coefficients.is_empty()
    }

    fn iter<'a>(&'a self) -> impl Iterator<Item = (f64, &'a T)> {
        self.weights.iter().cloned().zip(self.coefficients.iter())
    }
}

impl<T> iter::Extend<(f64, T)> for WeightedVecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn extend<II>(&mut self, iter: II)
    where
        II: IntoIterator<Item = (f64, T)>,
    {
        let weights = Rc::make_mut(&mut self.weights);

        for (weight, elem) in iter {
            match self
                .coefficients
                .values
                .binary_search_by(|value| value.cmp(&elem))
            {
                Ok(pos) => {
                    weights[pos] += weight;
                    if is_close(weights[pos], 0.) {
                        weights.remove(pos);
                        hash::MemoizedHash::make_mut(Rc::make_mut(
                            &mut self.coefficients.values,
                        ))
                        .remove(pos);
                    }
                }
                Err(pos) => {
                    weights.insert(pos, weight);
                    hash::MemoizedHash::make_mut(Rc::make_mut(
                        &mut self.coefficients.values,
                    ))
                    .insert(pos, elem);
                }
            }
        }
    }
}

impl<T> iter::FromIterator<(f64, T)> for WeightedVecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn from_iter<II>(iter: II) -> Self
    where
        II: IntoIterator<Item = (f64, T)>,
    {
        let mut values = Vec::from_iter(iter);
        values.sort_by(|(_, lhs), (_, rhs)| lhs.cmp(rhs));

        // TODO: sumdups()

        WeightedVecSet::from_sorted_iter(values)
    }
}

impl<T> WeightedVecSet<T>
where
    T: Ord + Clone + Hash,
{
    fn from_sorted_iter<II>(values: II) -> Self
    where
        II: IntoIterator<Item = (f64, T)>,
    {
        let (weights, coefficients): (Vec<_>, Vec<_>) = values.into_iter().unzip();
        WeightedVecSet {
            weights: Rc::new(weights),
            coefficients: VecSet::from_sorted_iter(coefficients),
        }
    }

    fn map_weights<F>(&self, f: F) -> WeightedVecSet<T>
    where
        F: FnMut(f64) -> f64,
    {
        WeightedVecSet {
            weights: Rc::new(self.weights.iter().cloned().map(f).collect()),
            coefficients: self.coefficients.clone(),
        }
    }
    fn union(&self, other: &WeightedVecSet<T>) -> WeightedVecSet<T> {
        if self.is_empty() {
            other.clone()
        } else {
            self.union_map(other, |rhs| rhs)
        }
    }

    fn union_map<F>(&self, other: &WeightedVecSet<T>, map_rhs: F) -> WeightedVecSet<T>
    where
        F: Fn(f64) -> f64,
    {
        if self.is_empty() {
            other.map_weights(map_rhs)
        } else if other.is_empty() {
            self.clone()
        } else if self.coefficients == other.coefficients {
            let mut weights = Vec::with_capacity(self.weights.len());
            let mut to_remove = Vec::new();

            for (ix, (lhs, rhs)) in self
                .weights
                .iter()
                .cloned()
                .zip(other.weights.iter().cloned())
                .enumerate()
            {
                let sum = lhs + map_rhs(rhs);
                if is_close(sum, 0.) {
                    to_remove.push(ix);
                } else {
                    weights.push(sum);
                }
            }

            let coefficients = if to_remove.is_empty() {
                self.coefficients.clone()
            } else {
                VecSet::from_sorted_iter(
                    self.coefficients
                        .iter()
                        .enumerate()
                        .merge_join_by(&to_remove, |(ix, _), rix| ix.cmp(rix))
                        .filter_map(|either| {
                            use itertools::EitherOrBoth::*;

                            match either {
                                Left((_, item)) => Some(item.clone()),
                                Right(rix) => {
                                    unreachable!("removing nonexistant index {}", rix)
                                }
                                Both(_, _) => None,
                            }
                        }),
                )
            };

            WeightedVecSet {
                weights: Rc::new(weights),
                coefficients,
            }
        } else {
            WeightedVecSet::from_sorted_iter(
                WeightedUnion::new(
                    self.iter(),
                    other
                        .iter()
                        .map(move |(weight, item)| (map_rhs(weight), item)),
                )
                .map(|(weight, item)| (weight, item.clone())),
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct FMinExpr<P>
where
    P: Atom,
{
    min: f64,
    max: f64,
    values: VecSet<DiffExpr<P>>,
}

impl<P> Eq for FMinExpr<P> where P: Atom + Eq {}

impl<P> Ord for FMinExpr<P>
where
    P: Atom + Ord,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl<P> Hash for FMinExpr<P>
where
    P: Atom + Hash,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.values.hash(state);
    }
}

impl<P> fmt::Display for FMinExpr<P>
where
    P: Atom,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "min[{} <= {}](", self.min, self.max)?;

        if self.values.is_empty() {
            debug_assert_eq!(self.min, self.max);

            write!(fmt, "{}", self.max)?;
        } else {
            let maxmin = self
                .values
                .iter()
                .skip(1)
                .map(DiffExpr::max_value)
                .fold(self.values[0].max_value(), f64::min);
            assert!(maxmin.is_finite());

            if maxmin != self.max {
                write!(fmt, "{}, ", self.max)?;
            }

            write!(
                fmt,
                "{}",
                self.values
                    .iter()
                    .map(|val| format!(
                        "[{} <= {}] {}",
                        val.min_value(),
                        val.max_value(),
                        val
                    ))
                    .format(", ")
            )?;
        }

        write!(fmt, ")")
    }
}

impl<P> From<FMinExpr<P>> for Float<P>
where
    P: Atom,
{
    fn from(emin: FMinExpr<P>) -> Self {
        emin.simplified()
            .unwrap_or_else(|emin| FloatInner::Min(emin).into())
    }
}

impl<P> From<f64> for FMinExpr<P>
where
    P: Atom,
{
    fn from(constant: f64) -> Self {
        Self::new(constant, constant, VecSet::default())
    }
}

impl<P> From<Float<P>> for FMinExpr<P>
where
    P: Atom,
{
    fn from(value: Float<P>) -> Self {
        match value.as_f64() {
            Some(val) => val.into(),
            None => match &*value {
                FloatInner::Min(emin) => emin.clone(),
                _ => FMinExpr::new(
                    value.min_value(),
                    value.max_value(),
                    VecSet::new(DiffExpr::from(value)),
                ),
            },
        }
    }
}

impl<'a, 'b, P> Add<&'a f64> for &'b FMinExpr<P>
where
    P: Atom,
{
    type Output = FMinExpr<P>;

    fn add(self, other: &'a f64) -> FMinExpr<P> {
        FMinExpr::new(
            self.min + other,
            self.max + other,
            self.values.iter().map(|item| item + other).collect(),
        )
    }
}

impl<P> FMinExpr<P>
where
    P: Atom,
{
    fn new(min: f64, max: f64, values: VecSet<DiffExpr<P>>) -> Self {
        assert!(min.is_finite() && max.is_finite());
        assert!(min <= max);

        FMinExpr { min, max, values }
    }

    fn fmin(&self, other: &FMinExpr<P>) -> FMinExpr<P> {
        if self.max <= other.min {
            self.clone()
        } else if other.max <= self.min {
            other.clone()
        } else {
            let min = self.min.min(other.min);
            let max = self.max.min(other.max);

            let values = VecSet::from_sorted_iter(
                Union::new(
                    self.values.iter().filter(|value| value.min_value() < max),
                    other.values.iter().filter(|value| value.min_value() < max),
                )
                .cloned(),
            );

            FMinExpr::new(min, max, values)
        }
    }

    fn is_constant(&self) -> bool {
        self.min == self.max
    }

    fn is_single_value(&self) -> bool {
        // Need to check that the min value is close to the min value of the single arg, because it
        // is possible that we computed a min with a constant value
        self.values.len() == 1 && is_close(self.values[0].max_value(), self.max)
    }

    fn simplified(self) -> Result<Float<P>, Self> {
        if self.is_constant() {
            debug_assert_eq!(self.min, self.max);

            Ok(self.min.into())
        } else if self.is_single_value() {
            debug_assert!(self.values.len() == 1);

            Ok(self.values.values[0].clone().into())
        } else {
            Err(self)
        }
    }
}
