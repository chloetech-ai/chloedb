#!/bin/bash
# Runs a command with no inherited Claude Code settings, memory or instructions.
exec env -i PATH="$PATH" HOME=/home/user/bench/persona/home ANTHROPIC_BASE_URL="$ANTHROPIC_BASE_URL" CHLOECTX_MODELS=/home/user/data/models TMPDIR=/home/user/bench/persona/tmp READER_CWD=/home/user/bench/persona/reader/cwd "$@"
