mod advice_taint;

pub use self::advice_taint::{
    AdviceTaintAnalysis, AdviceTaintDiagnostic, AdviceTaintFinding, AdviceTaintPropagation,
    AdviceTaintValue,
};
