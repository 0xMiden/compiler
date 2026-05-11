use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use miden_core::{serde::Deserializable, utils::ToHex};
use miden_mast_package::{Package, PackageExport};
use midenc_compile::{Compiler, Context};
use midenc_session::{
    InputFile, OutputType,
    diagnostics::{IntoDiagnostic, Report, WrapErr},
};

pub fn wasm_to_masm(
    wasm_file_path: &Path,
    output_folder: &Path,
    mut midenc_args: Vec<String>,
) -> Result<PathBuf, Report> {
    if !output_folder.exists() {
        return Err(Report::msg(format!(
            "MASM output folder '{}' does not exist.",
            output_folder.to_str().unwrap()
        )));
    }
    log::debug!(
        "Compiling '{}' Wasm to '{}' directory with midenc ...",
        wasm_file_path.to_str().unwrap(),
        &output_folder.to_str().unwrap()
    );
    let input = InputFile::from_path(wasm_file_path)
        .into_diagnostic()
        .wrap_err("Invalid input file")?;
    let masm_file_name = wasm_file_path
        .file_stem()
        .expect("invalid wasm file path: no file stem")
        .to_str()
        .unwrap();
    let output_file =
        output_folder.join(masm_file_name).with_extension(OutputType::Masp.extension());

    let mut args: Vec<String> = vec![
        "--output-dir".to_string(),
        output_folder.to_str().unwrap().to_string(),
        "-o".to_string(),
        output_file.to_str().unwrap().to_string(),
        "--verbose".to_string(),
    ];
    args.append(&mut midenc_args);

    log::debug!("midenc arguments: {}", &args.join(" "));
    let session = Rc::new(Compiler::new_session([input], None, args));
    let context = Rc::new(Context::new(session));
    println!("Creating Miden package {}", output_file.display());
    midenc_compile::compile(context.clone())?;
    print_package_exports(&output_file);
    Ok(output_file)
}

/// Prints exported procedure roots for a compiled Miden package.
fn print_package_exports(output_file: &Path) {
    let package_bytes = match fs::read(output_file) {
        Ok(package_bytes) => package_bytes,
        Err(err) => {
            eprintln!(
                "[miden package exports] context=cargo-miden compile_masm output={} error=failed \
                 to read package: {err}",
                output_file.display()
            );
            return;
        }
    };

    let package = match Package::read_from_bytes(&package_bytes) {
        Ok(package) => package,
        Err(err) => {
            eprintln!(
                "[miden package exports] context=cargo-miden compile_masm output={} error=failed \
                 to decode package: {err}",
                output_file.display()
            );
            return;
        }
    };

    println!(
        "[miden package exports] context=cargo-miden compile_masm output={} package={} exports={}",
        output_file.display(),
        package.name,
        package.manifest.num_exports()
    );

    for export in package.manifest.exports() {
        let PackageExport::Procedure(proc_export) = export else {
            continue;
        };

        println!(
            "[miden package export] context=cargo-miden compile_masm output={} package={} path={} \
             root={}",
            output_file.display(),
            package.name,
            proc_export.path,
            proc_export.digest.as_bytes().to_hex_with_prefix()
        );
    }
}
