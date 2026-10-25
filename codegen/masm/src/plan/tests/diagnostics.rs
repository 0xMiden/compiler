use alloc::{format, sync::Arc, vec};

use midenc_session::diagnostics::{
    DefaultSourceManager, DiagnosticsHandler, NullEmitter, PrintDiagnostic, SourceLanguage,
    SourceManager, Uri,
};

use super::*;

#[test]
fn report_renders_source_and_inline_ancestry() {
    let sources = Arc::new(DefaultSourceManager::default());
    let primary = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("selected.hir"),
        "selected_operation\ncaller_operation\n".into(),
    );
    let caller = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("caller.hir"),
        "outer_caller\n".into(),
    );
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    let mut error = PlanError::new(DiagnosticCategory::IllegalStackAccess, "invalid stack depth");
    error.origin = SourceLocation {
        span: crate::masm::SourceSpan::new(primary.id(), 0..18),
        inline_chain: vec![
            crate::masm::SourceSpan::new(primary.id(), 19..35),
            crate::masm::SourceSpan::new(caller.id(), 0..12),
        ],
    };
    error.step = Some(SelectedStepId(7));
    error.depth = Some(16);
    let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
    for expected in [
        "IllegalStackAccess",
        "step=s7",
        "depth=16",
        "selected.hir",
        "selected_operation",
        "caller_operation",
        "caller.hir",
        "outer_caller",
        "inlined from",
    ] {
        assert!(rendered.contains(expected), "missing {expected:?}: {rendered}");
    }
}

#[test]
fn display_keeps_context_without_raw_source_spans() {
    let mut error = PlanError::new(DiagnosticCategory::UnavailableValue, "missing lane");
    error.step = Some(SelectedStepId(4));
    error.depth = Some(2);
    error.expected = Some(Atom::new(ValueId(0), 0));
    error.actual = Some(Atom::new(ValueId(1), 0));
    let message = error.to_string();
    for expected in ["UnavailableValue", "step=s4", "depth=2", "expected=", "actual="] {
        assert!(message.contains(expected), "{message}");
    }
    assert!(!message.contains("SourceSpan"), "{message}");
}

#[test]
fn report_without_resolvable_source_has_useful_fallback() {
    let sources = Arc::new(DefaultSourceManager::default());
    let source =
        sources.load(SourceLanguage::Other("test"), Uri::new("short.hir"), "short\n".into());
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    for span in [
        crate::masm::SourceSpan::UNKNOWN,
        crate::masm::SourceSpan::new(crate::masm::SourceSpan::UNKNOWN.source_id(), 10..20),
        crate::masm::SourceSpan::new(source.id(), 10..20),
    ] {
        let mut error =
            PlanError::new(DiagnosticCategory::InvalidSelectedLowering, "missing source evidence");
        error.origin.span = span;
        let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
        assert!(rendered.contains("missing source evidence"), "{rendered}");
        assert!(rendered.contains("source location unavailable"), "{rendered}");
    }
}

#[test]
fn unavailable_primary_does_not_promote_inline_caller_to_failure_origin() {
    let sources = Arc::new(DefaultSourceManager::default());
    let caller = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("caller.hir"),
        "outer_caller\n".into(),
    );
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    let mut error = PlanError::new(DiagnosticCategory::UnavailableValue, "missing lane");
    error.origin.inline_chain.push(crate::masm::SourceSpan::new(caller.id(), 0..12));
    let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
    assert!(rendered.contains("UnavailableValue: missing lane"), "{rendered}");
    assert!(rendered.contains("source location unavailable"), "{rendered}");
    assert!(!rendered.contains("caller.hir"), "{rendered}");
}

#[test]
fn unavailable_inline_frame_keeps_usable_frames_and_their_ordinals() {
    let sources = Arc::new(DefaultSourceManager::default());
    let primary = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("selected.hir"),
        "selected_operation\n".into(),
    );
    let caller = sources.load(
        SourceLanguage::Other("test"),
        Uri::new("caller.hir"),
        "outer_caller\n".into(),
    );
    let diagnostics =
        DiagnosticsHandler::new(Default::default(), sources, Arc::new(NullEmitter::default()));
    let mut error = PlanError::new(DiagnosticCategory::UnavailableValue, "missing lane");
    error.origin = SourceLocation {
        span: crate::masm::SourceSpan::new(primary.id(), 0..18),
        inline_chain: vec![
            crate::masm::SourceSpan::UNKNOWN,
            crate::masm::SourceSpan::new(caller.id(), 0..12),
        ],
    };
    let rendered = format!("{}", PrintDiagnostic::new(error.into_report(&diagnostics)));
    for expected in [
        "selected.hir",
        "selected_operation",
        "caller.hir",
        "outer_caller",
        "inlined from (frame 2)",
        "one or more inline source locations unavailable",
    ] {
        assert!(rendered.contains(expected), "missing {expected:?}: {rendered}");
    }
}
