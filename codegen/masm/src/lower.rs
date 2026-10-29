mod component;
pub(crate) mod debug;
pub(crate) mod expand;
mod lowering;
mod native_ptr;
pub(crate) mod raw;
pub(crate) mod unit;
pub(crate) mod units;

pub(crate) use component::{EmissionOwners, finalize_generated};
pub(crate) use lowering::{HirSelection, invocation_target_from_symbol};

pub use self::{
    component::{EmissionNamespace, ToMasmComponent},
    native_ptr::NativePtr,
};

/// Resolve a bare planning failure at the session boundary, preserving other report context.
pub(crate) fn report_planning_failure(
    report: midenc_session::diagnostics::Report,
    diagnostics: &midenc_session::diagnostics::DiagnosticsHandler,
) -> midenc_session::diagnostics::Report {
    use crate::plan::PlanError;

    // Report::downcast traverses context wrappers. Only the immediate cause belongs
    // to this adapter; converting a wrapped failure would discard its outer context.
    if report.chain().next().is_some_and(|cause| cause.is::<PlanError>()) {
        match report.downcast::<PlanError>() {
            Ok(error) => error.into_report(diagnostics),
            Err(report) => report,
        }
    } else {
        report
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use alloc::{format, rc::Rc};

    use midenc_hir::{Context, diagnostics::Uri, dialects::builtin, pass::AnalysisManager};
    use midenc_session::diagnostics::PrintDiagnostic;

    use super::*;

    fn assert_authored_call_diagnostic(text: &str) {
        let context = Rc::new(Context::default());
        let op = midenc_hir::parse::parse_any(
            midenc_hir::parse::ParserConfig {
                context: context.clone(),
                verify: true,
            },
            Uri::new("authored-call.hir"),
            text,
        )
        .expect("fixture should parse");
        let manager = AnalysisManager::new(op, None);
        let result = if let Ok(world) = op.try_downcast_op::<builtin::World>() {
            world.borrow().to_masm_component(manager, EmissionNamespace::Source)
        } else {
            op.try_downcast_op::<builtin::Component>()
                .unwrap()
                .borrow()
                .to_masm_component(manager, EmissionNamespace::Source)
        };
        let error = result.err().expect("authored depth-sensitive helper must be rejected");
        let rendered = format!("{}", PrintDiagnostic::new(error));
        for expected in [
            "truncate_stack requires registered return cleanup",
            "authored-call.hir",
            "hir.exec @truncate_stack",
        ] {
            assert!(rendered.contains(expected), "missing {expected:?}: {rendered}");
        }
    }

    #[test]
    fn world_lowering_renders_authored_planning_failure_source() {
        assert_authored_call_diagnostic(
            r#"
            builtin.world {
                builtin.module public @miden {
                    builtin.module public @core {
                        builtin.module public @sys {
                            builtin.function public extern("C") @truncate_stack() { builtin.ret; };
                            builtin.function public extern("C") @main() {
                                hir.exec @truncate_stack() : extern("C") () -> ();
                                builtin.ret;
                            };
                        };
                    };
                };
            };
        "#,
        );
    }

    #[test]
    fn component_lowering_renders_authored_planning_failure_source() {
        assert_authored_call_diagnostic(
            r#"
            builtin.component private @miden::@core {
                builtin.module public @sys {
                    builtin.function public extern("C") @truncate_stack() { builtin.ret; };
                    builtin.function public extern("C") @main() {
                        hir.exec @truncate_stack() : extern("C") () -> ();
                        builtin.ret;
                    };
                };
            };
        "#,
        );
    }

    #[test]
    fn planning_report_adapter_preserves_wrapped_context() {
        use midenc_session::diagnostics::{DiagnosticsHandler, Report};

        let report = Report::from(crate::plan::PlanError::new(
            crate::plan::DiagnosticCategory::AbiMismatch,
            "inner planning failure",
        ))
        .wrap_err("outer lowering context");
        let adapted = report_planning_failure(report, &DiagnosticsHandler::default());
        let chain = adapted.chain().map(|cause| cause.to_string()).collect::<alloc::vec::Vec<_>>();
        assert_eq!(chain, ["outer lowering context", "AbiMismatch: inner planning failure"]);
        assert!(adapted.downcast_ref::<crate::plan::PlanError>().is_some());
    }

    #[test]
    fn planning_report_adapter_preserves_unrelated_reports() {
        use midenc_session::diagnostics::{DiagnosticsHandler, Report};

        let report = Report::msg("unrelated failure").wrap_err("outer context");
        let adapted = report_planning_failure(report, &DiagnosticsHandler::default());
        let chain = adapted.chain().map(|cause| cause.to_string()).collect::<alloc::vec::Vec<_>>();
        assert_eq!(chain, ["outer context", "unrelated failure"]);
        let rendered = format!("{}", PrintDiagnostic::new(adapted));
        assert!(rendered.contains("outer context"), "{rendered}");
        assert!(rendered.contains("unrelated failure"), "{rendered}");
        assert!(!rendered.contains("source location unavailable"), "{rendered}");
    }
    #[test]
    fn allocation_reporting_preserves_precise_source_and_uses_procedure_fallback() {
        use alloc::sync::Arc;

        use midenc_session::diagnostics::{
            DefaultSourceManager, DiagnosticsHandler, NullEmitter, SourceLanguage, SourceManager,
        };

        use crate::{
            allocate::state::{AllocationError, AllocationFailure},
            plan::*,
        };
        let sources = Arc::new(DefaultSourceManager::default());
        let source = sources.load(
            SourceLanguage::Other("test"),
            Uri::new("allocation.hir"),
            "procedure_body\nselected_operation\n".into(),
        );
        let procedure_span = crate::masm::SourceSpan::new(source.id(), 0..14);
        let unit_span = crate::masm::SourceSpan::new(source.id(), 15..33);
        let diagnostics =
            DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
        let name = crate::masm::ProcedureName::new("allocator_fixture").unwrap();
        for precise in [false, true] {
            let mut error =
                PlanError::new(DiagnosticCategory::UnavailableValue, "unrecoverable operand");
            if precise {
                error.origin.span = unit_span;
                error.step = Some(SelectedStepId(7));
                error.expected = Some(Atom::new(ValueId(3), 0));
            }
            let report = component::allocation_error(error.into(), &name, procedure_span);
            let error = report.downcast_ref::<PlanError>().unwrap();
            assert_eq!(error.origin.span, if precise { unit_span } else { procedure_span });
            let rendered =
                format!("{}", PrintDiagnostic::new(report_planning_failure(report, &diagnostics)));
            for expected in [
                "allocation.hir",
                "procedure allocator_fixture",
                "unrecoverable operand",
                if precise {
                    "selected_operation"
                } else {
                    "procedure_body"
                },
            ] {
                assert!(rendered.contains(expected), "missing {expected}: {rendered}");
            }
            if precise {
                assert!(
                    rendered.contains("step=s7") && rendered.contains("expected=v3.0"),
                    "{rendered}"
                );
            }
        }
        let mut error: AllocationError =
            PlanError::new(DiagnosticCategory::UnavailableValue, "unused").into();
        error.cause = AllocationFailure::IdentifierExhausted("stack occurrences");
        let report = component::allocation_error(error, &name, procedure_span);
        assert_eq!(
            report.downcast_ref::<PlanError>().unwrap().category,
            DiagnosticCategory::IdentifierExhausted
        );
        let rendered =
            format!("{}", PrintDiagnostic::new(report_planning_failure(report, &diagnostics)));
        assert!(
            rendered.contains("allocation.hir")
                && rendered.contains("stack occurrences")
                && rendered.contains("allocator_fixture"),
            "{rendered}"
        );
    }
}
