# Local tool-call smoke tests — Apple M4, 16 GB

## Ollama privado · ADR0039 (2026-11-11)

`seiya_ollama_private.py` es un adaptador manual macOS, no parte del servidor.
`make test` solo ejecuta sus fixtures offline; no arranca Ollama ni carga pesos.
Checker de presión/respuesta en Rust `seiya-ollama-check`, sin dependencias nuevas.
Ensayo vacío y protección de escritura comprobados; intento real Gemma abortó
por presión warning a3,224s, con cleanup confirmado. Qwen no se intentó; no retry
automático ni par concurrente validado. Operación, autorización y límites:
[`PRUEBAS_OLLAMA_PRIVADO.md`](../docs/PRUEBAS_OLLAMA_PRIVADO.md).
Con permiso posterior separado ADR0040, modo `gemma-calibration` completó una
respuesta E2B en4,416s (carga4,214s), warning transitorio contabilizado1,431s y
swap+139MiB, cleanup confirmado. No cambiar gate general, probar Qwen o admitir
concurrencia por este resultado breve; memoria total del modelo sigue sin medir.
Qwen recibió permisos individuales posteriores: ADR0041 paró por critical a3,296s;
ADR0042 permitió critical pero paró a3,543s por swap+872,7MiB (>512MiB). Cleanup
confirmado en ambos, sin respuesta validada/retries automáticos. No inferir que
el modo permisivo desactiva los demás límites o demuestra estabilidad.
Con permiso nuevo ADR0043, `qwen-observe-calibration` deja memoria informativa y
conserva plazo120s/aislamiento/validación/cleanup. Qwen respondió SEIYA_OK en4,859s,
carga4,008s y swap+470,75MiB, cleanup confirmado. No cambio del supervisor general
ni validación de sesiones largas, calidad de código o modelos simultáneos.

## Rust / OpenCode integration (2026-10-19)

`seiya_rust_opencode_smoke.py` is a **manual, real-inference opt-in** probe with
owned MLX/Rust processes, an isolated OpenCode profile and synthetic fixtures.
It does not stop existing services or download weights. Two short Gemma runs
completed read/edit/shell and 3 tests, with 5 SSE responses and no compaction.
Instructions, measurements and limitations: [`docs/PRUEBA_RUST_OPENCODE.md`](../docs/PRUEBA_RUST_OPENCODE.md).
`make test` runs only the synthetic evidence-checker tests, never this real probe.

## Memory baseline design (S2-04)

[`docs/BASELINE_MEMORIA.md`](../docs/BASELINE_MEMORIA.md) and
`contracts/benchmarks/memory-baseline/v0.1.0/plan.json` specify a future A/B
campaign, not measured RAM savings. Sprint 03 adds native probes with an owned-
process smoke and a simulated runner; the real campaign driver/launcher is
still pending. No model campaign is run by these plan tests.
`test_memory_baseline_plan.py` checks the plan's offline invariants only:
equal budgets, balanced order, missing metrics, process roles and safety gates.
These tests do not run `ps`, inspect user processes, load models or collect memory.

## Results (2026-10-18)

| Model / runtime | Total passes | Tool scenarios | Median read round trip | Median two-tool chain |
|---|---:|---:|---:|---:|
| Qwen2.5-Coder-7B-Instruct-4bit / MLX-LM | 3/18 | 0/15 | failed | failed |
| Qwen3.5-9B-4bit / MLX-LM | 18/18 | 15/15 | 2.75 s | 6.10 s |
| Gemma 4 E4B-it-4bit / MLX-LM | 18/18 | 15/15 | 1.28 s | 2.54 s |
| GPT-OSS-20B MXFP4 / Ollama | 18/18 | 15/15 | 3.15 s | 4.93 s |

Qwen2.5 returned JSON inside Markdown, not structured `tool_calls`; these
responses were not executed or repaired. Its three passes were the no-tool
control. This result applies to the checkpoint, prompt and server tested,
not to every possible Qwen2.5 tool-calling integration.

Gemma's exact model is E4B, not a nonexistent Gemma 4 7B/9B variant. GPT-OSS
was tested through Ollama because the installed MLX tokenizer did not detect
a Harmony tool parser. The unused MLX GPT-OSS weights are also downloaded;
they have not been deleted. The Ollama alias `local-gpt-oss-tools` shares the
weights of `gpt-oss:20b` and sets a 2,048-token context.

GPT-OSS caused swap to grow to about 4 GB during testing, with only about
0.66 GB reported available at one sampled point. It was unloaded afterward.
Gemma's sampled post-test swap was about 0.30 GB; Qwen3.5's was zero. These
are snapshots, not continuous peak-memory measurements, and sequential runs
carry over OS cache/memory history. All test model servers were stopped at
the end; the models remain on disk.

Versions: MLX-LM 0.32.2, MLX 0.32.4, Transformers 5.20.1, Ollama 0.40.6.

## Codestral Mamba 7B follow-up

Checkpoint: `mlx-community/Mamba-Codestral-7B-v0.1-4bit`, about 4.10 GB.
It loaded using the installed `mamba2` implementation, but the conversion
provides neither a chat template nor a detected tool parser. The default
MLX chat endpoint passed the no-tool control and failed all five tool scenarios
(1/6 total, one repetition). It wrote descriptions or fabricated results
instead of producing structured calls. This is an unsupported integration,
not evidence that every adaptation of the original model must fail.

To avoid judging coding on the generic chat fallback alone, three specialist
tasks and three review choices used explicit `<s>[INST] ... [/INST]` prompts
through `/v1/completions`. See `probe_mamba.py`:

- Merge intervals: invalid Python syntax.
- LRU cache: invalid Python syntax and incomplete implementation.
- SQLite task store: invalid Python syntax and incomplete implementation.
- SQL injection review: correct choice B, but invalid strict JSON output.
- LRU review: valid JSON, but wrong choice A and an incorrect justification
  claiming `OrderedDict.get` updates recency.
- Age-boundary review: correct choice A, but invalid strict JSON output.

Only surrounding Markdown fences were removed for syntax checks. No generated
code was fixed or executed. A separate in-process generation compared decoded
token IDs to streamed text: the syntax corruption was present in both, so it
was not merely an HTTP formatting artifact. The outputs differed in other
details between runs. That probe measured about 21.5 generation tokens/s and
5.43 GB MLX peak memory; it is not a sustained or end-to-end benchmark.

Raw outputs and `validation.json` are in
`~/colibri-models/tool-test-results/codestral-mamba-7b/`. This local 4-bit
checkpoint/runtime combination is not recommended as the third agent based
on these probes. The original weights and another runtime have not been
evaluated; do not generalize this to all Codestral models. Its server was
stopped afterward, and the downloaded checkpoint remains on disk.

## Third-family comparison and authorized cleanup

The user subsequently authorized testing all three research candidates and
removing unsuccessful candidates. Qwen2.5-Coder-7B MLX and Codestral Mamba 7B
MLX weight directories were removed; their raw results remain. Earlier notes
above describe the state at the time of those tests, not current retention.

| Candidate | Runtime/settings | Tool suite | Functional code tasks | Strict JSON reviews |
|---|---|---:|---:|---:|
| Ministral 3 8B Instruct 2512, 4-bit | MLX, temperature 0 | 12/18 | 3/3 | 0/3 |
| LFM2.5-8B-A1B, official MLX 4-bit | MLX, temperature .2, top_p .95 | 18/18 | 1/3 | 3/3 |
| Granite 4.2 3B Q4_K_M | Ollama, temperature 1, top_p .95, reasoning_effort none | 17/18 | 0/3 | 3/3 |

The prompts/tasks were the same across these three candidates. Sampling and
output budgets differed to accommodate manufacturer recommendations and
reasoning overhead. These tiny samples are not statistically robust rankings.
The code tasks are merge_intervals, LRUCache, and SQLite TaskStore. Review
choices cover SQL injection, LRU recency and an age boundary. `validate_code.py`
runs fixed independent tests after manual inspection; it is NOT a sandbox and
must never be used on unreviewed generated code. Code is not repaired to pass.
Complete outer Markdown fences may be stripped for Python syntax/execution;
review JSON is judged strictly, without stripping fences.

Details:

- Ministral made valid real tool calls in every tool scenario, including the
  two dependent calls, but lost the random suffix of the returned shipping
  status in 3/3 chain responses. It also returned `HOLA.` rather than `HOLA`
  in 3/3 controls. All three generated implementations passed functional
  tests. Review choices were semantically correct, but were fenced in Markdown;
  the LRU explanation also contained unescaped line breaks in a JSON string.
  It remains a coding-specialist candidate, NOT a clean 18/18 tool worker.
  Its downloaded tokenizer configuration received the Transformers-recommended
  `fix_mistral_regex: true` setting before testing; weights/template untouched.

- Liquid passed tools but used substantial reasoning even with the MLX
  enable_thinking=false setting. Initial coding/review probes truncated at
  1,000/384 output tokens. A separate `code-expanded` run used 3,072/1,536
  tokens: LRU passed; intervals incorrectly merged separated intervals;
  TaskStore contained invalid syntax and additional semantic mistakes.
  Expanded reviews passed all three strict JSON/choice checks. Retained as a
  tool/coordinator/reviewer candidate, not a trusted code implementer.

- Granite initially scored 16/18 because the Ollama endpoint ignored MLX's
  chat_template_kwargs and left reasoning on; two recovery requests truncated.
  With documented reasoning_effort=none and a 2,048-token output budget,
  17/18 passed. The lone failure obtained the correct data but skipped the
  explicitly requested first attempt to read the nonexistent file. Its code
  duplicated intervals, evicted the most-recent LRU item, and returned a
  generator instead of a task list (also missing required SQL defaults/commits).
  Review answers were correct but did not predict its own generation quality.
  The imported Ollama model and source GGUF directory were removed after testing.

Ministral and Liquid weights were retained because they work for different
roles, despite not being universally reliable. All their servers are stopped.
Gemma E4B and Qwen3.5-9B remain unchanged. No multi-agent voting system has
been implemented, and no full OpenCode integration was tested.

## What was tested

Six scenarios, each repeated three times at temperature zero:

1. A direct response with tools available but unnecessary.
2. Read a file containing a fresh random value, then return the actual value.
3. Write exact content and verify the resulting file outside the model.
4. Look up an order, then use its unpredictable tracking ID in a second tool.
5. Recover from a missing file by listing files, then reading an available one.
6. Streaming tool-call assembly followed by an actual tool-result round trip.

The harness checks call IDs, function names, required string arguments,
`finish_reason`, observable file effects and actual returned data. It executes
only fixed allowlisted Python handlers inside temporary workspaces. It never
executes model-generated code or shell commands. Raw replies and results are
under `~/colibri-models/tool-test-results/`; dependencies are isolated in
`~/colibri-models/mlx-tests-venv/`.

These are smoke tests, not a comprehensive agent benchmark. Three greedy
repetitions with varied random payloads are not independent stochastic trials.
They do not establish coding quality, long-context reliability, prompt-injection
resistance, parallel tool calls, arbitrary JSON-schema compliance, forced
`tool_choice`, MCP or full OpenCode integration. Ollama documents that
`tool_choice` is unsupported by its OpenAI compatibility endpoint. We have not
changed the user's OpenCode provider configuration.

## Reproduce

Run one MLX server at a time (example: Gemma):

```bash
~/colibri-models/mlx-tests-venv/bin/mlx_lm.server \
  --model ~/colibri-models/mlx-gemma4-e4b \
  --host 127.0.0.1 --port 8081 \
  --decode-concurrency 1 --prompt-concurrency 1 \
  --prefill-step-size 256 --prompt-cache-size 1 \
  --prompt-cache-bytes 256MB --max-tokens 384 \
  --chat-template-args '{"enable_thinking":false}'
```

In another terminal, from the repository root:

```bash
~/colibri-models/mlx-tests-venv/bin/python local-tools-tests/run_suite.py \
  --model "$HOME/colibri-models/mlx-gemma4-e4b" \
  --output "$HOME/colibri-models/tool-test-results/gemma4-new-run"
```

GPT-OSS uses a larger output budget for its native reasoning and a different
runtime, so timings are deployment comparisons, not controlled comparisons
of model architecture or decode throughput:

```bash
ollama create local-gpt-oss-tools -f local-tools-tests/Modelfile.gpt-oss-tests
~/colibri-models/mlx-tests-venv/bin/python local-tools-tests/run_suite.py \
  --url http://127.0.0.1:11434 --model local-gpt-oss-tools \
  --output "$HOME/colibri-models/tool-test-results/gpt-oss-new-run" \
  --max-tokens 1024 --reasoning-effort low
ollama stop local-gpt-oss-tools
```

## Multi-agent recommendation (not implemented/tested)

Use a sequential orchestrator rather than keeping multiple models resident.
Gemma E4B is the quickest tested tool worker; Qwen3.5-9B is another working
tool-capable candidate. A coordinator can pass a short structured plan to an
executor, then pass the diff and test results to a reviewer. The roles may
also be separate invocations of the same model, avoiding model reloads.

Only permit scoped tools and let deterministic tests decide acceptance. Bound
the number of review rounds and tool calls. Such a workflow is not weight
merging, does not sum parameters, may repeat shared errors and is not proven
to outperform a single model. Establish single-agent coding baselines first.
