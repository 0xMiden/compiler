#!/bin/bash
# Post-edit hook: builds the modified contract if a file in contracts/ was edited.
# Reads JSON input from stdin (Claude Code hook protocol).

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.filePath // empty')

# Only trigger for files inside this project's contracts/ directory
if [[ -z "$FILE_PATH" ]] || [[ "$FILE_PATH" != "$CLAUDE_PROJECT_DIR/contracts/"* ]]; then
  exit 0
fi

# Find the contract directory (parent of src/)
CONTRACT_DIR=$(echo "$FILE_PATH" | sed 's|/src/.*||')
CARGO_TOML="$CONTRACT_DIR/Cargo.toml"

if [[ ! -f "$CARGO_TOML" ]]; then
  exit 0
fi

# Detect which build tool is available (midenup installs `miden`, cargo install provides `cargo-miden`)
if command -v miden &> /dev/null; then
  BUILD_CMD="miden build"
elif cargo miden --version &> /dev/null; then
  BUILD_CMD="cargo miden build"
else
  echo '{"hookSpecificOutput": {"additionalContext": "Contract build skipped: neither '\''miden'\'' nor '\''cargo-miden'\'' found. Install via midenup or: cargo install cargo-miden"}}'
  exit 0
fi

# Run build once, capturing output
BUILD_OUTPUT=$($BUILD_CMD --manifest-path "$CARGO_TOML" --release 2>&1)
BUILD_EXIT=$?

if [[ $BUILD_EXIT -eq 0 ]]; then
  echo '{"hookSpecificOutput": {"additionalContext": "Contract build succeeded"}}'
  exit 0
else
  TAIL_OUTPUT=$(echo "$BUILD_OUTPUT" | tail -20)
  jq -n --arg ctx "Contract build FAILED. Fix compilation errors before continuing."$'\n'"$TAIL_OUTPUT" \
    '{"hookSpecificOutput": {"additionalContext": $ctx}}'
  exit 2
fi
