# kde-mcp latency profile — 7 days ending 2026-05-18

N = 3 tool calls across 2 tools.

_Illustrative sample from light development use — the value here is the per-phase profiling methodology (see `tools/latency-analyzer/`), not the sample size._

| Tool          |    n | p50 ms | p95 ms | p99 ms | dominant phase (p95)
|---------------|-----:|-------:|-------:|-------:|---------------------
| window_query  |    1|      3|      3|      3| impl
| observe       |    2|      2|      3|      3| impl

## Phase-level p95 (ms)

| Tool          | rmcp | policy | impl | audit
|---------------|-----:|-------:|-----:|-----:
| window_query  |    0|      0|    3|     0
| observe       |    0|      0|    3|     0
