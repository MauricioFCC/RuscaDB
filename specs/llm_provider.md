---
id: SPEC-0100
feature: llm_provider
status: implemented
owner: harness-team
appetite_days: 1
boundaries:
  crates: [xtask]
  files: [scripts/llm_provider.py, scripts/llm_judge.py, crates/xtask/tests/llm_provider.rs]
  out_of_scope: [backend LLM en CI, descarga de modelos, juicio semántico de specs, reescribir T2]
fr:
  - { id: FR-0100-01, desc: "scripts/llm_provider.py resuelve provider/model/base_url/key por precedencia CLI > entorno > default y expone complete(prompt, system=None) -> str (solo stdlib)" }
  - { id: FR-0100-02, desc: "un único protocolo OpenAI-compatible para local (llama.cpp) y openai: POST {base}/chat/completions, Bearer solo si hay key, contenido en choices[0].message.content" }
  - { id: FR-0100-03, desc: "openai exige OPENAI_API_KEY (exit 2 accionable si falta); local (llama.cpp) y ollama nunca la exigen" }
  - { id: FR-0100-04, desc: "ollama legacy: POST {base}/api/chat con stream=false y contenido en message.content, sin key" }
  - { id: FR-0100-05, desc: "scripts/llm_judge.py construye el prompt T2 (Spec Fidelity >= 0.90 + revisión adversarial, 0 hallazgos HIGH); --show-prompt lo imprime sin red" }
  - { id: FR-0100-06, desc: "--show-config imprime {provider, base_url, model, api_key_present} sin red y sin exigir key a local/ollama" }
nf:
  - { id: NF-0100-01, desc: "solo stdlib (urllib.request/json/os/sys); --show-config y --show-prompt no tocan la red" }
  - { id: NF-0100-02, desc: "fallo ruidoso: configuración inválida o error de red/HTTP => exit 2 con WHAT/WHY/WHERE (nunca verde silencioso)" }
  - { id: NF-0100-03, desc: "docstrings ES con Args/Returns/Raises en toda función/clase pública" }
acceptance_criteria:
  - id: AC-0100-01
    given: el entorno limpio (sin variables RUSCADB_LLM_* ni OPENAI_*)
    when: se ejecuta llm_provider.py --show-config
    then: el proveedor por defecto es local (llama.cpp) en http://127.0.0.1:8080/v1, con el modelo por defecto y api_key_present=false
    test: test_ac_0100_01_show_config_defaults_local
  - id: AC-0100-02
    given: RUSCADB_LLM_PROVIDER=openai y OPENAI_API_KEY definida
    when: se ejecuta --show-config (y luego con --provider local)
    then: openai usa https://api.openai.com/v1 con gpt-4o-mini y api_key_present=true; --provider local sobreescribe el entorno
    test: test_ac_0100_02_show_config_openai_env_override
  - id: AC-0100-03
    given: RUSCADB_LLM_PROVIDER=openai sin OPENAI_API_KEY
    when: se ejecuta --show-config
    then: sale con exit 2 y un mensaje accionable que nombra OPENAI_API_KEY; local nunca la exige
    test: test_ac_0100_03_openai_missing_key_exits_two
  - id: AC-0100-04
    given: RUSCADB_LLM_PROVIDER=ollama
    when: se ejecuta --show-config
    then: usa el puerto nativo http://127.0.0.1:11434 y api_key_present=false
    test: test_ac_0100_04_show_config_ollama_legacy
  - id: AC-0100-05
    given: un diff y RUSCADB_LLM_PROVIDER=openai sin key
    when: se ejecuta llm_judge.py --show-prompt <diff>
    then: sale 0 e imprime el prompt T2 (Spec Fidelity >= 0.90 + 0 hallazgos HIGH) sin resolver el proveedor ni tocar la red
    test: test_ac_0100_05_judge_show_prompt_offline
exit_criteria:
  - cargo test -p xtask --test llm_provider
  - cargo clippy -p xtask --all-targets -- -D warnings
  - cargo xtask trace verde (spec implemented trazada)
rollback:
  - retirar scripts/llm_provider.py, scripts/llm_judge.py y crates/xtask/tests/llm_provider.rs (herramienta del harness, sin efecto en el producto)
sandbox:
  - cargo test -p xtask --test llm_provider
---

# SPEC-0100 — Cliente LLM agnóstico de proveedor para el harness T2

## Contexto

La puerta T2 (SPEC-0061) definió el juez determinista y dejó el **backend LLM**
como futuro. Para cablearlo (LLM-judge, revisión de código multi-agente) hace
falta un cliente que no acople el harness a un backend concreto: en desarrollo
se corre un modelo local y en producción un modelo cloud, sin cambiar el código
que invoca.

El estado del arte 2026 converge en el **protocolo OpenAI-compatible** como
denominador común: `llama.cpp` (`llama-server`), vLLM, llama-swap y la propia
API de OpenAI exponen `POST {base}/chat/completions` con
`choices[0].message.content`. Por eso `local` (llama.cpp, **desarrollo**) y
`openai` (**producción**) comparten un único protocolo y solo difieren en la
configuración; `ollama` (protocolo nativo `POST {base}/api/chat`) se mantiene
como **compatibilidad legacy** para no romper setups existentes.

El harness selecciona el backend **por entorno**: `RUSCADB_LLM_PROVIDER=openai`
+ `OPENAI_API_KEY` en producción; por defecto `local` (llama.cpp en
`http://127.0.0.1:8080/v1`, sin exigir key).

## Superficie

- `scripts/llm_provider.py`: resolución de configuración por precedencia
  `--provider/--model/--base-url` > entorno > default; `complete(prompt,
  system=None) -> str`; `--show-config` (JSON sin red) y `--prompt`.
  - `local` (DEFAULT): `RUSCADB_LLM_BASE_URL` (default
    `http://127.0.0.1:8080/v1`), `RUSCADB_LLM_MODEL` (default
    `qwen3-4b-thinking-2507-q4-k-m`), key opcional `RUSCADB_LLM_API_KEY`.
  - `openai`: `OPENAI_BASE_URL` (default `https://api.openai.com/v1`),
    `OPENAI_API_KEY` obligatoria, `RUSCADB_LLM_MODEL` (default `gpt-4o-mini`).
  - `ollama` (legacy): `RUSCADB_LLM_BASE_URL` (default
    `http://127.0.0.1:11434`), sin key.
- `scripts/llm_judge.py`: prompt T2 (Spec Fidelity `>= 0.90` + revisión
  adversarial, 0 hallazgos HIGH); `--show-prompt` imprime sin red.
- Tests: `crates/xtask/tests/llm_provider.rs`.

Queda fuera: cablear el backend en CI, descargar modelos y el juicio semántico
de specs.

## Criterios de aceptación

- **AC-0100-01** — default `local`/llama.cpp (8080/v1, sin key).
- **AC-0100-02** — override `openai` por entorno (con key) y `--provider`.
- **AC-0100-03** — `openai` sin `OPENAI_API_KEY` => exit 2 accionable.
- **AC-0100-04** — `ollama` legacy (11434, sin key).
- **AC-0100-05** — `--show-prompt` offline con el prompt T2.

## Notas de diseño

- Un solo protocolo moderno (OpenAI-compatible) para `local` y `openai`; la
  `Authorization: Bearer` se añade solo si hay key, de modo que un
  `llama-server` sin auth funciona igual.
- Precedencia uniforme CLI > entorno > default, con la base y la key resueltas
  por proveedor (tabla `_PROVIDER_SPECS`).
- `--show-config` expone `api_key_present` (nunca la clave) para diagnosticar
  la configuración sin red; `openai` sin key falla ruidoso (exit 2).

## Trazabilidad

Tests `test_ac_0100_<nn>_*` con marcador `@spec AC-0100-NN`; verificado por
`cargo xtask trace`/`contract`.
