use core::{any::TypeId, cell::RefCell};

use compact_str::CompactString;
use smallvec::SmallVec;

use super::OperationPass;
use crate::{OperationName, OperationRef};

#[allow(unused_variables)]
pub trait PassInstrumentation {
    fn run_before_pipeline(
        &mut self,
        name: Option<&OperationName>,
        parent_info: &PipelineParentInfo,
    ) {
    }
    fn run_after_pipeline(
        &mut self,
        name: Option<&OperationName>,
        parent_info: &PipelineParentInfo,
    ) {
    }
    fn run_before_pass(&mut self, pass: &dyn OperationPass, op: &OperationRef) {}
    fn run_after_pass(&mut self, pass: &dyn OperationPass, op: &OperationRef) {}
    fn run_after_pass_failed(&mut self, pass: &dyn OperationPass, op: &OperationRef) {}
    fn run_before_analysis(&mut self, name: &str, id: &TypeId, op: &OperationRef) {}
    fn run_after_analysis(&mut self, name: &str, id: &TypeId, op: &OperationRef) {}
}

pub struct PipelineParentInfo {
    /// The pass that spawned this pipeline, if any
    pub pass: Option<CompactString>,
}

impl<P: ?Sized + PassInstrumentation> PassInstrumentation for Box<P> {
    fn run_before_pipeline(
        &mut self,
        name: Option<&OperationName>,
        parent_info: &PipelineParentInfo,
    ) {
        (**self).run_before_pipeline(name, parent_info);
    }

    fn run_after_pipeline(
        &mut self,
        name: Option<&OperationName>,
        parent_info: &PipelineParentInfo,
    ) {
        (**self).run_after_pipeline(name, parent_info);
    }

    fn run_before_pass(&mut self, pass: &dyn OperationPass, op: &OperationRef) {
        (**self).run_before_pass(pass, op);
    }

    fn run_after_pass(&mut self, pass: &dyn OperationPass, op: &OperationRef) {
        (**self).run_after_pass(pass, op);
    }

    fn run_after_pass_failed(&mut self, pass: &dyn OperationPass, op: &OperationRef) {
        (**self).run_after_pass_failed(pass, op);
    }

    fn run_before_analysis(&mut self, name: &str, id: &TypeId, op: &OperationRef) {
        (**self).run_before_analysis(name, id, op);
    }

    fn run_after_analysis(&mut self, name: &str, id: &TypeId, op: &OperationRef) {
        (**self).run_after_analysis(name, id, op);
    }
}

#[derive(Default)]
pub struct PassInstrumentor {
    instrumentations: RefCell<SmallVec<[Box<dyn PassInstrumentation>; 1]>>,
}

impl PassInstrumentor {
    pub fn run_before_pipeline(
        &self,
        name: Option<&OperationName>,
        parent_info: &PipelineParentInfo,
    ) {
        self.instrument(|pi| pi.run_before_pipeline(name, parent_info));
    }

    pub fn run_after_pipeline(
        &self,
        name: Option<&OperationName>,
        parent_info: &PipelineParentInfo,
    ) {
        self.instrument(|pi| pi.run_after_pipeline(name, parent_info));
    }

    pub fn run_before_pass(&self, pass: &dyn OperationPass, op: &OperationRef) {
        self.instrument(|pi| pi.run_before_pass(pass, op));
    }

    pub fn run_after_pass(&self, pass: &dyn OperationPass, op: &OperationRef) {
        self.instrument(|pi| pi.run_after_pass(pass, op));
    }

    pub fn run_after_pass_failed(&self, pass: &dyn OperationPass, op: &OperationRef) {
        self.instrument(|pi| pi.run_after_pass_failed(pass, op));
    }

    pub fn run_before_analysis(&self, name: &str, id: &TypeId, op: &OperationRef) {
        self.instrument(|pi| pi.run_before_analysis(name, id, op));
    }

    pub fn run_after_analysis(&self, name: &str, id: &TypeId, op: &OperationRef) {
        self.instrument(|pi| pi.run_after_analysis(name, id, op));
    }

    pub fn add_instrumentation(&self, pi: Box<dyn PassInstrumentation>) {
        self.instrumentations.borrow_mut().push(pi);
    }

    #[inline(always)]
    fn instrument<F>(&self, callback: F)
    where
        F: Fn(&mut dyn PassInstrumentation),
    {
        let mut instrumentations = self.instrumentations.borrow_mut();
        for pi in instrumentations.iter_mut() {
            callback(pi);
        }
    }
}
