mod advice_taint;

pub use self::advice_taint::{
    AdviceTaintAnalysis, AdviceTaintDiagnostic, AdviceTaintExitFinding, AdviceTaintFinding,
    AdviceTaintOrigin, AdviceTaintOriginKind, AdviceTaintPropagation, AdviceTaintValue,
};
