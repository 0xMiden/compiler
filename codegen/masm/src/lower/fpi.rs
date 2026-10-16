//! Foreign procedure invocation lowering support.
//!
//! FPI calls are executed by the transaction kernel procedure
//! `miden::protocol::tx::execute_foreign_procedure`. Its protocol ABI is fixed-width: each call
//! receives 22 felt operands. The first 6 operands are the account/procedure prefix (account id
//! suffix, account id prefix, and the procedure root word), and the remaining 16 operands are the
//! flattened procedure inputs. The executor returns one felt for each possible procedure input
//! slot.
//!
//! The frontend represents every FPI call as a single `hir.exec_fpi` op whose operands are just
//! the real procedure input felts, while the 6 prefix felts are stored in function locals
//! referenced by the op. This keeps the operand stack shallow enough that lowering never has to
//! shuffle past the 16-element addressable window: the scheduled inputs are padded with zeroes to
//! the fixed 16-felt input width, and the prefix is then loaded from the locals on top of the
//! padded inputs to form the full 22-felt executor ABI.

use midenc_dialect_hir as hir;
use midenc_hir::{
    CallConv, Op, Type,
    dialects::builtin::attributes::{LocalVariable, Signature},
};
use midenc_session::diagnostics::{Report, Spanned};

use super::lowering::{HirLowering, invocation_target_from_symbol_path};
use crate::{emitter::BlockEmitter, opt::operands::MASM_STACK_WINDOW_FELTS};

// The executor's input slots are padded as a call window is.
const _: () = assert!(hir::ExecFpi::MAX_INPUT_FELTS == MASM_STACK_WINDOW_FELTS);

impl HirLowering for hir::ExecFpi {
    fn emit(&self, emitter: &mut BlockEmitter<'_>) -> Result<(), Report> {
        let span = self.span();
        let num_inputs = self.inputs().len();
        if num_inputs > hir::ExecFpi::MAX_INPUT_FELTS {
            return Err(Report::msg(format!(
                "`hir.exec_fpi` received {num_inputs} procedure input operands, but accepts at \
                 most {}",
                hir::ExecFpi::MAX_INPUT_FELTS
            )));
        }
        let prefix_locals = {
            let prefix_locals = self.get_prefix_locals();
            if prefix_locals.len() != hir::ExecFpi::PREFIX_FELTS {
                return Err(Report::msg(format!(
                    "`hir.exec_fpi` references {} prefix locals, but requires exactly {}",
                    prefix_locals.len(),
                    hir::ExecFpi::PREFIX_FELTS
                )));
            }
            prefix_locals
                .iter()
                .copied()
                .map(|local| (local, emitter.frame.locaddr(&local)))
                .collect::<smallvec::SmallVec<[(LocalVariable, u16); 6]>>()
        };

        let mut inst_emitter = emitter.inst_emitter(self.as_operation());

        // Pad the scheduled procedure inputs with zeroes, the trailing input slots, up to the
        // fixed executor input width, which is the width of a call window.
        inst_emitter.pad_operands_to_call_window(num_inputs, span);

        // Load the executor prefix from the locals on top of the padded inputs. The locals hold
        // the prefix in executor operand order, so they are pushed in reverse to leave the first
        // operand on top of the stack.
        for (local, offset) in prefix_locals.iter().rev() {
            inst_emitter.load_local(local, *offset, span);
        }

        let signature = Signature::with_convention(
            &inst_emitter.context_rc(),
            CallConv::Wasm,
            vec![Type::Felt; hir::ExecFpi::EXECUTOR_INPUT_FELTS],
            vec![Type::Felt; hir::ExecFpi::EXECUTOR_RESULT_FELTS],
        );
        let callee =
            invocation_target_from_symbol_path(&hir::ExecFpi::executor_symbol_path(), span);
        inst_emitter.exec(callee, &signature, span);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        SymbolPath, TraceTarget, Type, ValueRef, dialects::builtin::BuiltinOpBuilder,
        formatter::PrettyPrint, pass::AnalysisManager, testing::Test,
    };
    use midenc_hir_analysis::analyses::LivenessAnalysis;

    use super::*;
    use crate::{linker::LinkInfo, stack::OperandStack};

    /// The inputs are padded with the call-window padding (whole words where the input count
    /// allows it), and the six prefix locals are loaded on top before the executor is invoked.
    #[test]
    fn exec_fpi_pads_inputs_and_loads_prefix_locals() -> Result<(), Report> {
        // (inputs, `padw`, `push.0`, the instruction moving inputs and padding past each other)
        for (num_inputs, padw, push_zero, shuffle) in [
            (0usize, 4usize, 0usize, None),
            (1, 3, 3, Some(("movup.15", 1))),
            (4, 3, 0, Some(("movupw.3", 1))),
            (8, 2, 0, Some(("swapdw", 1))),
            (10, 0, 6, Some(("movdn.10", 6))),
            (12, 1, 0, Some(("movdnw.3", 1))),
            (15, 0, 1, Some(("movdn.15", 1))),
        ] {
            let output = emit_exec_fpi(num_inputs)?.to_pretty_string();

            // The prefix local loads are direct `locaddr`/`mem_load` pairs and push nothing else.
            assert_eq!(
                output.matches("padw").count(),
                padw,
                "unexpected padding words for {num_inputs} inputs:\n{output}"
            );
            assert_eq!(
                output.matches("push.0").count(),
                push_zero,
                "unexpected padding felts for {num_inputs} inputs:\n{output}"
            );
            if let Some((shuffle, count)) = shuffle {
                assert_eq!(
                    output.matches(shuffle).count(),
                    count,
                    "unexpected input padding shuffle for {num_inputs} inputs:\n{output}"
                );
            }
            assert_eq!(
                output.matches("mem_load").count(),
                6,
                "exec_fpi must load all six prefix locals:\n{output}"
            );
            // The locals are loaded in reverse executor order, so the last local is loaded first.
            for index in 0..6 {
                let locaddr = format!("locaddr.{}", 5 - index);
                assert!(
                    output.contains(&locaddr),
                    "exec_fpi must load prefix local {index}:\n{output}"
                );
            }
            assert!(
                output.contains("exec.::miden::protocol::tx::execute_foreign_procedure"),
                "exec_fpi must invoke the protocol executor:\n{output}"
            );
        }

        Ok(())
    }

    /// Sixteen inputs fill the executor's input slots, so nothing is padded or moved.
    #[test]
    fn exec_fpi_full_width_inputs_need_no_padding() -> Result<(), Report> {
        let output = emit_exec_fpi(16)?.to_pretty_string();

        // The prefix local loads are direct `locaddr`/`mem_load` pairs, so nothing pushes zeroes.
        assert_eq!(
            output.matches("push.0").count(),
            0,
            "full-width exec_fpi inputs must not be padded:\n{output}"
        );
        assert!(
            !output.contains("movdn."),
            "full-width exec_fpi inputs must not be shuffled:\n{output}"
        );
        assert!(
            output.contains("exec.::miden::protocol::tx::execute_foreign_procedure"),
            "exec_fpi must invoke the protocol executor:\n{output}"
        );

        Ok(())
    }

    fn emit_exec_fpi(num_inputs: usize) -> Result<crate::masm::Block, Report> {
        let params = vec![Type::Felt; num_inputs];
        let mut test = Test::new("exec_fpi_test", &params, &[]);
        let function_ref = test.function();
        let span = function_ref.span();
        let exec = {
            let mut builder = test.function_builder();
            let prefix_locals: [LocalVariable; hir::ExecFpi::PREFIX_FELTS] =
                core::array::from_fn(|_| builder.alloc_local(Type::Felt));
            let entry = builder.entry_block();
            let args = {
                let entry = entry.borrow();
                entry.arguments().iter().copied().map(|arg| arg as ValueRef).collect::<Vec<_>>()
            };
            let exec = builder.exec_fpi(prefix_locals, args, span)?;
            builder.ret(core::iter::empty::<ValueRef>(), span)?;
            exec
        };

        let analysis_manager = AnalysisManager::new(function_ref.as_operation_ref(), None);
        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;
        let link_info = LinkInfo::new(Some(SymbolPath::from_masm_module_id("root")));
        let mut invoked = BTreeSet::default();
        let mut stack = OperandStack::new(test.context_rc());
        for _ in 0..num_inputs {
            stack.push(Type::Felt);
        }

        // The prefix locals are loaded through the frame, so it has to be the real one.
        let (local_offsets, num_locals) = {
            let function = function_ref.borrow();
            let locals_required =
                function.locals().iter().map(|ty| ty.size_in_felts()).sum::<usize>();
            (
                crate::emitter::local_offsets(&function),
                u16::try_from(locals_required).expect("too many locals"),
            )
        };

        let mut emitter = BlockEmitter {
            module_owner: None,
            frame: crate::emitter::FrameLayout::new(&local_offsets, num_locals),
            liveness: &liveness,
            emit_inline_calls: false,
            link_info: &link_info,
            invoked: &mut invoked,
            target: Default::default(),
            stack,
            trace_target: TraceTarget::category("codegen"),
        };

        HirLowering::emit(&*exec.borrow(), &mut emitter)?;
        Ok(emitter.into_emitted_block(span))
    }
}
