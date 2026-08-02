#!/bin/bash
# Check if the provider credential variable is set in this child environment
# The harness bash tool should have stripped it via env_clear() + SAFE_SHELL_ENV_KEYS allowlist
if [ -z "$UMANS_AI_CODING_PLAN_API_KEY" ]; then
    echo "CANARY_OK: UMANS_AI_CODING_PLAN_API_KEY is absent from child environment"
    exit 0
else
    echo "CANARY_FAIL: UMANS_AI_CODING_PLAN_API_KEY is present (length: ${#UMANS_AI_CODING_PLAN_API_KEY})"
    exit 1
fi
