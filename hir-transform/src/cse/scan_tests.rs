use alloc::{boxed::Box, format, rc::Rc, string::String};
use core::cell::Cell;

use midenc_hir::{
    Context, Report,
    diagnostics::Uri,
    dialects::builtin::Function,
    parse::{self, ParserConfig},
    pass::{Nesting, PassManager},
    print::AsmPrinter,
};

std::thread_local! { pub(super) static SCANS: Cell<usize> = const { Cell::new(0) }; }

fn run(source: &str) -> Result<(String, usize), Report> {
    let context = Rc::new(Context::default());
    let function =
        parse::parse::<Function>(ParserConfig::new(context.clone()), Uri::new("scan.hir"), source)?;
    let mut passes = PassManager::on::<Function>(context.clone(), Nesting::Implicit);
    passes.add_pass(Box::<super::CommonSubexpressionElimination>::default());
    SCANS.set(0);
    passes.run(function.as_operation_ref())?;
    function.as_operation_ref().borrow().recursively_verify()?;
    let flags = Default::default();
    let mut printer = AsmPrinter::new(context, &flags);
    printer.print_operation(function.borrow());
    Ok((format!("{}", printer.finish()), SCANS.get()))
}

#[test]
fn surviving_separators_require_linear_memory_scan_work() -> Result<(), Report> {
    let mut excessive_work = alloc::vec::Vec::new();
    for debug in [false, true] {
        for count in [32, 64, 128] {
            let mut source = String::from(
                r#"builtin.function public extern("C") @scan(%seed: u32) -> u32 {
                hir.store_local %seed <{ local = #builtin.local_variable<0, u32> }> : (u32);
                %r0 = hir.load_local <{ local = #builtin.local_variable<0, u32> }>;
                %a0 = arith.add %seed, %r0 <{ overflow = #builtin.overflow<wrapping> }>;
"#,
            );
            for i in 1..=count {
                source += &format!(
                    "%r{i} = hir.load_local <{{ local = #builtin.local_variable<0, u32> \
                     }}>;\n%a{i} = arith.add %a{}, %r{i} <{{ overflow = \
                     #builtin.overflow<wrapping> }}>;\n",
                    i - 1
                );
                if debug {
                    source += &format!(
                        "di.debug_value %a{i} <{{ variable = #di.variable<{{ name = \"sum\", file \
                         = \"test.rs\", line = 1 }}>, expression = #di.expression<[]> }}> : \
                         (u32);\n"
                    );
                }
            }
            source += &format!("builtin.ret %a{count} : (u32); }};");
            let (output, scans) = run(&source)?;
            assert_eq!(output.matches("hir.load_local").count(), 1);
            assert_eq!(output.matches("arith.add").count(), count + 1);
            std::println!("debug={debug} duplicates={count} effect_queries={scans}");
            if scans > 4 * count {
                excessive_work.push((debug, count, scans));
            }
        }
    }
    assert!(excessive_work.is_empty(), "expected linear scan work: {excessive_work:?}");
    Ok(())
}
