use alloc::boxed::Box;

use midenc_dialect_hir::transforms::LiftControlFlowToSCF;
use midenc_hir2::{
    pass::{Nesting, PassManager},
    transforms::Canonicalizer,
};

use super::*;

/// This stage applies all registered (and enabled) module-scoped rewrites to input HIR module(s)
pub struct ApplyRewritesStage;
impl Stage for ApplyRewritesStage {
    type Input = LinkOutput;
    type Output = LinkOutput;

    fn enabled(&self, context: &Context) -> bool {
        !context.session().options.link_only
    }

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        log::debug!("applying rewrite passes");
        // TODO(pauls): Set up pass registration for new pass infra
        /*
        // Get all registered module rewrites and apply them in the order they appear
        let mut registered = vec![];
        let matches = context.session().matches();
        for rewrite in inventory::iter::<RewritePassRegistration<hir::Module>> {
            log::trace!("checking if flag for rewrite pass '{}' is enabled", rewrite.name);
            let flag = rewrite.name();
            if matches.try_contains_id(flag).is_ok() {
                if let Some(index) = matches.index_of(flag) {
                    let is_enabled = matches.get_flag(flag);
                    if is_enabled {
                        log::debug!(
                            "rewrite pass '{}' is registered and enabled",
                            rewrite.name
                        );
                        registered.push((index, rewrite.get()));
                    }
                }
            }
        }
        registered.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
        */

        // Construct a pass manager with the default pass pipeline
        let mut pm = PassManager::on::<builtin::World>(context.clone(), Nesting::Implicit);

        // Component passes
        {
            let mut component_pm = pm.nest::<builtin::Component>();
            // Function passes for module-level functions
            {
                let mut module_pm = component_pm.nest::<builtin::Module>();
                let mut func_pm = module_pm.nest::<builtin::Function>();
                func_pm.add_pass(Canonicalizer::create());
                func_pm.add_pass(Box::new(LiftControlFlowToSCF));
                // Re-run canonicalization to clean up generated structured control flow
                func_pm.add_pass(Canonicalizer::create());
            }
            // Function passes for component-level functions
            {
                let mut func_pm = component_pm.nest::<builtin::Function>();
                func_pm.add_pass(Canonicalizer::create());
                func_pm.add_pass(Box::new(LiftControlFlowToSCF));
                // Re-run canonicalization to clean up generated structured control flow
                func_pm.add_pass(Canonicalizer::create());
            }
        }

        // Run pass pipeline
        pm.run(input.world.as_operation_ref())?;

        log::debug!("rewrites successful");

        if context.session().rewrite_only() {
            log::debug!("stopping compiler early (rewrite-only=true)");
            Err(CompilerStopped.into())
        } else {
            Ok(input)
        }
    }
}
