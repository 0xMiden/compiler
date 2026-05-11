mod advice_taint;

pub use self::advice_taint::{
    AdviceTaintAnalysis, AdviceTaintContext, AdviceTaintContextKind, AdviceTaintDiagnostic,
    AdviceTaintExitFinding, AdviceTaintExternalCallFinding, AdviceTaintFinding, AdviceTaintOrigin,
    AdviceTaintOriginKind, AdviceTaintPropagation, AdviceTaintValue,
};
