"""Phase 0 capability smoke for the configured computer-use API tool.

Run explicitly with ``python -m harness.smoke`` or ``harness-smoke``.
This is a paid API request, not a full tool execution/result round trip.
Permission, text-only, and network failures must be resolved before
building the rest of the evaluation harness.
"""
from __future__ import annotations
import os
from dataclasses import dataclass


@dataclass
class SmokeResult:
    ok: bool
    status: str  # "ok" | "no-api-key" | "permission_denied" | "text-only" | "transient" | "other"
    detail: str = ""


def classify_smoke_result(response) -> SmokeResult:
    """Parse a successful Anthropic response into a SmokeResult.

    A successful smoke is `stop_reason == "tool_use"` with at least one
    `tool_use` content block. Anything else is suspect.
    """
    content = getattr(response, "content", [])
    has_tool_use = any(getattr(b, "type", None) == "tool_use" for b in content)
    if has_tool_use:
        return SmokeResult(ok=True, status="ok",
                           detail=f"stop_reason={response.stop_reason}")
    return SmokeResult(
        ok=False, status="text-only",
        detail=f"stop_reason={response.stop_reason}; no tool_use block",
    )


def run_smoke() -> SmokeResult:
    """Execute the live smoke. Returns a SmokeResult."""
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        return SmokeResult(ok=False, status="no-api-key",
                           detail="set ANTHROPIC_API_KEY in env")

    from anthropic import Anthropic, APIStatusError, APIConnectionError
    client = Anthropic(api_key=api_key)

    try:
        # Step 1: ask Claude to take a screenshot.
        resp = client.beta.messages.create(
            model="claude-opus-4-7",
            max_tokens=1024,
            tools=[{
                "type": "computer_20251124", "name": "computer",
                "display_width_px": 1920, "display_height_px": 1080,
            }],
            betas=["computer-use-2025-11-24"],
            messages=[{"role": "user", "content": "Take a screenshot."}],
        )
    except APIStatusError as e:
        status = getattr(e, "status_code", 0)
        msg = str(e).lower()
        if "permission" in msg or "not granted" in msg or status == 403:
            return SmokeResult(ok=False, status="permission_denied",
                               detail=str(e)[:300])
        return SmokeResult(ok=False, status="other", detail=str(e)[:300])
    except APIConnectionError as e:
        return SmokeResult(ok=False, status="transient", detail=str(e)[:300])

    return classify_smoke_result(resp)


def main() -> int:
    result = run_smoke()
    print(f"smoke: {result.status}")
    if result.detail:
        print(f"  detail: {result.detail}")
    print(f"  ok: {result.ok}")
    return 0 if result.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
