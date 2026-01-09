import os
import sys
import shlex

from lit.formats import ShTest
import lit.util

config.name = "miden-lit"
config.test_format = ShTest()
config.suffixes = [".shtest", ".hir", ".wat"]

source_root = os.path.dirname(__file__)
repo_root = os.path.abspath(os.path.join(source_root, os.pardir, os.pardir))
config.test_source_root = source_root
config.test_exec_root = repo_root
bin_dir = os.path.join(repo_root, "bin")
config.environment["PATH"] = bin_dir + os.pathsep + config.environment.get("PATH", "")
# Use cargo run to ensure proper runtime environment
# Redirect cargo's stderr to suppress build warnings, but keep midenc's stderr
midenc_cmd = f"cargo run --manifest-path {shlex.quote(os.path.join(repo_root, 'Cargo.toml'))} --bin midenc 2>/dev/null --"
config.substitutions.append(("%midenc", midenc_cmd))

# Try to find FileCheck in common locations
filecheck = (
    lit.util.which("FileCheck")
    or lit.util.which("filecheck")
    or lit.util.which("llvm-filecheck")
)

# Check homebrew LLVM locations if not found
if not filecheck:
    homebrew_paths = [
        "/opt/homebrew/opt/llvm@20/bin/FileCheck",
        "/opt/homebrew/opt/llvm/bin/FileCheck",
        "/usr/local/opt/llvm/bin/FileCheck",
    ]
    for path in homebrew_paths:
        if os.path.exists(path):
            filecheck = path
            break

# Fall back to simple_filecheck.py only if system FileCheck not found
if not filecheck:
    script = os.path.join(source_root, 'tools', 'simple_filecheck.py')
    filecheck = f"{shlex.quote(sys.executable)} {shlex.quote(script)}"

config.substitutions.append(("%filecheck", filecheck))

config.substitutions.append(("%S", source_root))

config.environment.setdefault("RUSTFLAGS", "")
