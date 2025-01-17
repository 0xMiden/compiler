use midenc_hir::pass::AnalysisManager;
use midenc_session::Session;

use crate::{CompilerResult, CompilerStopped};

/// This trait is implemented by a stage in the compiler
pub trait Stage {
    type Input;
    type Output;

    /// Return true if this stage is disabled
    fn enabled(&self, _session: &Session) -> bool {
        true
    }

    /// Run this stage
    fn run(
        &mut self,
        input: Self::Input,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output>;

    fn next<S>(self, stage: S) -> Chain<Self, S>
    where
        Self: Sized,
        S: Stage<Input = Self::Output>,
    {
        Chain::new(self, stage)
    }

    fn next_optional<S>(self, stage: S) -> ChainOptional<Self, S>
    where
        Self: Sized,
        S: Stage<Input = Self::Output, Output = Self::Output>,
    {
        ChainOptional::new(self, stage)
    }

    fn collect<S, I>(self, stage: S) -> Collect<Self, S, I>
    where
        Self: Sized,
        I: IntoIterator<Item = Self::Input>,
        S: Stage<Input = Vec<Self::Output>>,
    {
        Collect::new(self, stage)
    }
}

impl<I, O> Stage for &mut dyn FnMut(I, &mut AnalysisManager, &Session) -> CompilerResult<O> {
    type Input = I;
    type Output = O;

    #[inline]
    fn run(
        &mut self,
        input: Self::Input,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output> {
        (*self)(input, analyses, session)
    }
}

impl<I, O> Stage for Box<dyn FnMut(I, &mut AnalysisManager, &Session) -> CompilerResult<O>> {
    type Input = I;
    type Output = O;

    #[inline]
    fn run(
        &mut self,
        input: Self::Input,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output> {
        self(input, analyses, session)
    }
}

/// This struct is used to chain multiple [Stage] together
pub struct Chain<A, B> {
    a: A,
    b: B,
}
impl<A, B> Chain<A, B> {
    fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}
impl<A, B> Stage for Chain<A, B>
where
    A: Stage,
    B: Stage<Input = <A as Stage>::Output>,
{
    type Input = <A as Stage>::Input;
    type Output = <B as Stage>::Output;

    fn run<'a>(
        &mut self,
        input: Self::Input,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output> {
        if !self.a.enabled(session) {
            return Err(CompilerStopped.into());
        }
        let output = self.a.run(input, analyses, session)?;
        if !self.b.enabled(session) {
            return Err(CompilerStopped.into());
        }
        self.b.run(output, analyses, session)
    }
}

/// This struct is used to chain two [Stages] together when the second might be disabled
pub struct ChainOptional<A, B> {
    a: A,
    b: B,
}
impl<A, B> ChainOptional<A, B> {
    fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}
impl<A, B> Stage for ChainOptional<A, B>
where
    A: Stage,
    B: Stage<Input = <A as Stage>::Output, Output = <A as Stage>::Output>,
{
    type Input = <A as Stage>::Input;
    type Output = <B as Stage>::Output;

    fn run<'a>(
        &mut self,
        input: Self::Input,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output> {
        if !self.a.enabled(session) {
            return Err(CompilerStopped.into());
        }
        let output = self.a.run(input, analyses, session)?;
        if !self.b.enabled(session) {
            Ok(output)
        } else {
            self.b.run(output, analyses, session)
        }
    }
}

/// This stage joins multiple inputs into a single output
pub struct Collect<A, B, I> {
    spread: A,
    join: B,
    _marker: core::marker::PhantomData<I>,
}
impl<A, B, I> Collect<A, B, I>
where
    A: Stage,
    B: Stage<Input = Vec<<A as Stage>::Output>>,
    I: IntoIterator<Item = <A as Stage>::Input>,
{
    pub fn new(spread: A, join: B) -> Self {
        Self {
            spread,
            join,
            _marker: core::marker::PhantomData,
        }
    }
}
impl<A, B, I> Stage for Collect<A, B, I>
where
    A: Stage,
    B: Stage<Input = Vec<<A as Stage>::Output>>,
    I: IntoIterator<Item = <A as Stage>::Input>,
{
    type Input = I;
    type Output = <B as Stage>::Output;

    fn run(
        &mut self,
        inputs: Self::Input,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> CompilerResult<Self::Output> {
        let mut outputs = vec![];
        for input in inputs.into_iter() {
            outputs.push(self.spread.run(input, analyses, session)?);
        }
        self.join.run(outputs, analyses, session)
    }
}
