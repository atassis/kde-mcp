"""Pytest configuration shared across the harness test suite."""
import os
import pytest


@pytest.fixture
def isolated_env(monkeypatch):
    """Strip env vars that could leak into tests (API keys, session tags)."""
    for var in ("ANTHROPIC_API_KEY", "KDE_MCP_SESSION_TAG",
                "BUDGET_SESSION_USD"):
        monkeypatch.delenv(var, raising=False)
    yield monkeypatch
