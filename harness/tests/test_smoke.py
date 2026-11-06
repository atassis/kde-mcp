"""Tests for the Phase 0 smoke. Live API call is gated behind
HARNESS_LIVE_SMOKE=1; offline tests cover the parsing/classification
of various failure modes."""
import pytest
from harness.smoke import classify_smoke_result, SmokeResult


def test_classify_text_only_response_is_failure():
    """If Claude returns only text and no tool_use, the beta header
    was likely accepted but the model decided not to call the tool —
    not a definitive grant, surface for human review."""
    fake_response = type("R", (), {
        "stop_reason": "end_turn",
        "content": [type("B", (), {"type": "text", "text": "I'd take a screenshot."})()],
    })()
    result = classify_smoke_result(fake_response)
    assert result.status == "text-only"
    assert not result.ok


def test_classify_tool_use_response_is_success():
    fake_response = type("R", (), {
        "stop_reason": "tool_use",
        "content": [
            type("B", (), {"type": "text", "text": "Taking a screenshot."})(),
            type("B", (), {"type": "tool_use", "name": "computer",
                           "input": {"action": "screenshot"}})(),
        ],
    })()
    result = classify_smoke_result(fake_response)
    assert result.status == "ok"
    assert result.ok


def test_smoke_result_is_dataclass_like():
    r = SmokeResult(ok=True, status="ok", detail="")
    assert r.ok
    assert r.status == "ok"


@pytest.mark.skipif(
    not __import__("os").environ.get("HARNESS_LIVE_SMOKE"),
    reason="set HARNESS_LIVE_SMOKE=1 explicitly for a paid Anthropic API call"
)
def test_live_smoke():
    """Makes an explicitly opted-in, paid Anthropic API request."""
    from harness.smoke import run_smoke
    result = run_smoke()
    print(f"\nlive smoke result: {result}")
    assert result.ok, f"smoke failed: {result.status} — {result.detail}"


def test_smoke_console_entry_point_targets_implemented_module():
    import pathlib
    import tomllib
    project = pathlib.Path(__file__).resolve().parents[1] / "pyproject.toml"
    scripts = tomllib.loads(project.read_text())["project"]["scripts"]
    assert scripts == {"harness-smoke": "harness.smoke:main"}


def test_smoke_without_api_key_stays_offline(monkeypatch):
    from harness.smoke import run_smoke
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    result = run_smoke()
    assert not result.ok
    assert result.status == "no-api-key"
