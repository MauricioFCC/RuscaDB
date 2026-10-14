# Verificación de RuscaDB — gates T1/T2/T3 y golden path

Este documento es el **golden path** de verificación (roadmap §7.1 y §6.5,
SPEC-0016). Describe qué valida cada tier, qué bloquea el merge y **cómo correr
cada gate localmente** antes de pushear.

Regla de oro: nada entra a `main` sin **T1 verde ∧ T2 verde ∧ spec trazada**.

```
T1  determinista   < 90 s    bloquea merge   .github/workflows/ci.yml
T2  LLM-judge      < 10 min  bloquea merge   local + revisión
T2m mutación-diff  variable  bloquea merge   ci.yml (job mutation-diff, solo PR)
T3  regression     < 60 min  alert-only      .github/workflows/nightly.yml
```

---

## T1 — determinista (bloquea merge, < 90 s)

Mismos comandos que ejecuta `.github/workflows/ci.yml` en cada push/PR a `main`.

```bash
# 1) Formato y lints
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 2) Tests (unit + integración)
cargo test --workspace --all-features --locked

# 3) Guardrails de arquitectura y unsafe
python scripts/check_architecture.py   # hexagonal: 0 ciclos, 0 aristas prohibidas
python scripts/check_core_unsafe.py    # política de unsafe y presupuestos

# 4) Trazabilidad SDD (spec -> test)
cargo xtask trace
cargo xtask contract   # marcadores @spec y estados de specs (SPEC-0061)

# 5) Config de CI/supply chain (SPEC-0016)
python scripts/check_ci_config.py

# 6) Supply chain
cargo deny check          # advisories + licenses + bans + sources (deny.toml)
cargo audit               # RustSec advisories
```

`--all-features` activa la feature opcional `datafusion` (spike `TableProvider`
de SPEC-0060, OFF por defecto, ADR-013): CI la compila una vez y la amortiza
con rust-cache; el árbol normal no la incluye (AC-0060-03).

Estado de referencia (actualizado 2026-10-14): **94 specs implementadas, 405
AC trazados** (`cargo xtask trace`), **555+ tests** (`cargo test --workspace --
--list`, sin `--all-features`).

Atajo opcional (mismos comandos, vía pre-commit):

```bash
pre-commit install
pre-commit run --all-files   # gitleaks + cargo fmt + cargo clippy
```

**Fitness functions** relevantes (roadmap §4.7): FF-01/FF-02 (grafo hexagonal),
FF-04 (`unsafe`), FF-08 (mutation score), FF-10 (dependencias), FF-11 (specs).

### Cross-platform (SPEC-0033/SPEC-0046) — T1

El job `test` de `ci.yml` corre sobre una **matriz OS rápida** en paralelo
(ubuntu + windows) para no bloquear el PR con la cola de runners de macOS:

```yaml
strategy:
  fail-fast: false
  matrix:
    os: [ubuntu-latest, windows-latest]
runs-on: ${{ matrix.os }}
```

Mismos comandos por runner (`cargo test --workspace --all-features --locked`);
el workspace debe ser portable (sin rutas de SO ni separadores hardcodeados).
`fail-fast: false` evita que un SO rojo cancele la evidencia del otro.
`scripts/check_ci_config.py` valida que la matriz declare ubuntu+windows.

**macOS se valida en T3** (`nightly.yml`, job `test-macos`) por `schedule`: la
cola de runners macOS es larga (~20 min) y no debe retrasar el feedback de
push/PR. Ver la sección T3.

### Higiene de release y MSRV (SPEC-0089/SPEC-0090) — T1

Dos jobs adicionales en `ci.yml`:

- **`commits`** (solo `pull_request`): corre `scripts/check_commits.py
  --range origin/<base>..HEAD` (Conventional Commits) y
  `scripts/check_changelog.py CHANGELOG.md` (sección `[Unreleased]`
  documentada). Falla ruidoso (exit 2) si no hay commits que revisar.
- **`msrv`**: compila con la toolchain del piso (`rust-version` = 1.89) y
  `cargo check --workspace --all-features --locked`, para que el MSRV no sea
  una promesa sin verificar. La versión vive en un solo lugar (`Cargo.toml`);
  `scripts/check_ci_config.py` compara el job contra ella.

El **release** lo publica `.github/workflows/release.yml` al empujar un tag
`v*` (SPEC-0089): valida la sección `[X.Y.Z]` del CHANGELOG, corre los guards,
genera el SBOM CycloneDX y crea el GitHub Release con las notas extraídas del
CHANGELOG. Mínimo privilegio: `contents: read` global, `contents: write` solo
en el job de publicación.

```bash
# Local: los mismos validadores que CI
python scripts/check_commits.py --range origin/main..HEAD
python scripts/check_changelog.py CHANGELOG.md --tag v0.1.0 --notes-out notas.md
python scripts/check_ci_config.py
```

### Escritura estructurada MCP (SPEC-0091) — T1

La fachada gana `Database::update(table, id, scalars)` (actualización parcial de
una fila viva por `RecordId`, reutilizando la ruta de índices del
`UPDATE ... WHERE`) y el servidor MCP publica las tools `update`/`delete` solo
con `McpServer::read_write`:

```bash
cargo test -p ruscadb --test row_update_api            # AC-0091-01
cargo test -p ruscadb --lib row_update                 # unit de la API
cargo test -p ruscadb-mcp --test mcp_write_tools_advanced  # AC-0091-02..04
```

Contrato: el adapter **no compone SQL** (identificadores/valores no inyectables);
el permiso real lo aplica el RBAC de la `Database` (rol `Read` ⇒ `-32000`) y un
`RecordId` inválido responde `-32602`. En solo lectura las tools responden un
rechazo accionable y no aparecen en `tools/list`.

### Reporte de cobertura de ramas (SPEC-0092) — T1

El job `coverage` publica un reporte explícito de ramas (BRDA) por crate
reutilizando el `lcov.info` del gate (sin re-ejecutar la suite):

```bash
python scripts/branch_coverage_report.py lcov.info \
  --scope ruscadb-core/ --min-branch 70 \
  --scope ruscadb-txn/ --min-branch 70 \
  --scope ruscadb-wal/ --min-branch 70 \
  --json-out branch-coverage.json
```

`scripts/check_ci_config.py` (`check_branch_report_job`) valida el cableado y
`crates/xtask/tests/branch_coverage.rs` cubre AC-0092-01/02.

### Persistencia del caché semántico (SPEC-0093) — T1

`Database::close` persiste el caché KNN en `<data>.cache.json` (versión 1,
`tmp`+`rename`) y `Database::open` lo restaura *best-effort*; una mutación
retira el sidecar y una base cifrada no persiste ni carga el caché en claro.

```bash
cargo test -p ruscadb --test semantic_cache_persistence   # AC-0093-01..04
```

### Gate RED-first (SPEC-0094) — T1 (solo PR)

Un spec en curso (`status: planned`/`draft`) tocado por el PR debe tener sus
tests citados por los ACs **en rojo**: `cargo xtask red <spec>` ejecuta
`cargo test --exact` (acotado a los crates del spec), clasifica la salida
(`Red`/`AlreadyPassing`/`Missing`) y el job `red-first` lo aplica a los specs
en curso del diff vía `scripts/red_first_gate.py --diff pr.diff`.

```bash
cargo test -p xtask --test red_first                 # AC-0094-02..04 (CLI/script/wiring)
cargo xtask red specs/<feature>.md                   # local: exige RED
python scripts/red_first_gate.py --list              # specs en curso
```

---

## T2 — LLM-judge (bloquea merge, < 10 min)

Se ejecuta sobre el **diff del PR** antes del merge:

- **Spec Fidelity ≥ 0.90**: el cambio satisface su `specs/<feature>.md`.
- **Behavioural spec** (pre/post) y **adversarial review** (0 hallazgos HIGH).
- **Mutantes in-diff** (`MS_diff ≥ 70 %`):

```bash
# Genera el diff del PR y mide los mutantes que toca.
git diff origin/main...HEAD > pr.diff
cargo mutants --in-diff pr.diff --in-place --baseline=skip
```

- **Judge** `repeat:3`, `temperature=0`, mayoría ≥ 2/3 (ver SPEC del LLM-judge).

### T2m — mutación sobre el diff (bloquea merge, SPEC-0061)

Parte determinista y bloqueante de T2: el job `mutation-diff` de `ci.yml`
(solo `pull_request`) muta únicamente el diff contra la base y exige
`MS_diff >= 70 %` vía `scripts/mutation_diff_gate.py`:

```bash
git diff origin/main...HEAD > pr.diff
cargo mutants --in-diff pr.diff --in-place --baseline=skip -o mutants.out
python scripts/mutation_diff_gate.py mutants.out/outcomes.json --min-score 70
```

Sin mutantes puntuables en el diff (p. ej. solo docs) el gate pasa con nota;
bajo el umbral falla con el conteo (matados/totales). `check_ci_config.py`
valida el cableado del job. Puerta solo-PR: en push a `main` no hay base
contra la que medirse.

### Política de cobertura (bloquea merge, SPEC-0061 + SPEC-0079)

El job `coverage` de `ci.yml` corre la suite instrumentada con
`cargo llvm-cov` en ubuntu y aplica `scripts/coverage_gate.py` sobre el
`lcov.info` con un scope por familia de crates y su umbral propio:

```bash
cargo llvm-cov --workspace --all-features --locked --lcov --output-path lcov.info
python scripts/coverage_gate.py lcov.info \
  --scope ruscadb-core/ --min-line 80  --min-branch 70 \
  --scope ruscadb-txn/  --min-line 90  --min-branch 70 \
  --scope ruscadb-wal/  --min-line 90  --min-branch 70
```

Medición de referencia (2026-10-13, artefacto `coverage-lcov` del run verde
`edb4d77`): core 96.4 %, `ruscadb-txn/` 96.5 %, `ruscadb-wal/` 96.6 %,
`ruscadb-query/` 93.8 %, `ruscadb-vector/` 94.7 %, `ruscadb-fvs/` 99.2 %,
`ruscadb-fts/` 99.8 % (línea; rama n/a: el export lcov no lleva BRDA, se
activa añadiendo `--branch-coverage` a `cargo llvm-cov`).

Semántica del gate (contrato SPEC-0061, conservado por SPEC-0079): la salida
imprime el % de línea/rama **por crate** dentro de cada scope para ver qué
crate arrastra el número; exit `0` = todos los scopes cumplen, exit `1` =
algún scope bajo umbral (nombra al infractor), exit `2` = scope sin datos o
lcov vacío: fallo ruidoso, nunca verde silencioso. Umbrales: core es el SLO
del roadmap (80/70); txn/wal son *hard floors* a 90 con ~6 pts de holgura
sobre lo medido — un descenso real de cobertura los rompe, ruido legítimo no.

### Gates estadísticos SLO (bloquea merge en PR, SPEC-0082)

Los runners de GitHub tienen CV ≈ 2.7 %, así que gatear «latencia ≤
baseline × 1.10» o «recall ≥ 0.93» con umbrales fijos produce falsos
positivos. `scripts/slo_gate.py` (solo stdlib, sin red) sustituye umbrales
por **tests de hipótesis** con contrato de salida 0/1/2 (SPEC-0079):

| Modo | Test estadístico | Uso |
| --- | --- | --- |
| `mann-whitney` | U unilateral con continuidad y empates, **p < 0.002** (Bonferroni ≈ 25 métricas) | latencias independientes |
| `bootstrap` | 10 000 remuestreos pareados, semilla fija, CI 99.8 % | recall@10 sobre los mismos pares de consulta |
| `aa` | Mann-Whitney entre dos muestras de la misma build | calibración del noise floor (cero falsos positivos) |

Cómo correrlo local:

```bash
python scripts/slo_gate.py benches/baselines/slo/latency_same.json      # => 0
python scripts/slo_gate.py benches/baselines/slo/latency_regression.json # => 1
python scripts/slo_gate.py benches/baselines/slo/malformed.json          # => 2
# Dos corridas de criterion (misma máquina), test z de Welch direccional:
cargo bench -p ruscadb-wal --bench wal_commit -- --sample-size 10
cp -r target/criterion criterion_base   # (tras la corrida base)
python scripts/slo_gate.py --criterion-pair criterion_base criterion_head --n 10
```

En CI, el workflow `.github/workflows/slo.yml` corre en cada PR: primero el
smoke de los fixtures versionados (segundos) y después dos corridas de
criterion — **base del PR y HEAD en el mismo runner** (comparación
intra-máquina, sin ruido entre máquinas) — y aplica
`--criterion-pair`. Exit `1` nombra el bench o la métrica infractor; exit `2`
es entrada malformada o baseline ausente: fallo ruidoso, nunca verde
silencioso. La suite `crates/ruscadb/tests/slo_gates.rs` asienta los tres
AC con los fixtures.

---

## T3 — regression nightly (alert-only, < 60 min)

Corre en `.github/workflows/nightly.yml` (`schedule` diario + `workflow_dispatch`).
**No bloquea PRs**: todos los jobs son `continue-on-error: true` y suben
artifacts. Un fallo T3 abre un issue *blocking* (roadmap §7.1).

### Mutation full sharded (MS documentado ≥ 85 %, FF-08)

```bash
# Shard local k de N (N=8 en CI); repetir k=0..N-1 o usar la matriz del workflow.
cargo mutants --workspace --shard=0/8 --in-place
# Reporte: mutants.out/outcomes.json  (artifact mutants-outcomes-<shard>)
```

Umbral T3: **mutation score ≥ 85 %** (merge T2: ≥ 70 %). Alert-only.

### Fuzzing continuo (libFuzzer, nightly)

```bash
rustup toolchain install nightly
cargo install cargo-fuzz

cd fuzz
cargo +nightly fuzz run query_parse  -- -max_total_time=300
cargo +nightly fuzz run wal_recover  -- -max_total_time=300
```

Invariantes (roadmap §6.2): `query_parse` y `wal_recover` nunca entran en pánico
ante entradas arbitrarias. Exit criteria: fuzz sin crash 7 días (§6.5).

### Miri — UB/`unsafe` en crates sin `unsafe`

```bash
rustup toolchain install nightly --component miri
cargo +nightly miri setup
cargo +nightly miri test -p ruscadb-query -p ruscadb-fts -p ruscadb-core
```

### Sanitizers — AddressSanitizer (SPEC-0033)

Detecta errores de memoria (use-after-free, buffer overflow) en `ruscadb-ffi`,
el único crate con `unsafe` permitido. Corre en nightly con `rust-src`:

```bash
rustup toolchain install nightly --component rust-src
RUSTFLAGS="-Zsanitizer=address" ASAN_OPTIONS=detect_leaks=0 \
  cargo +nightly test -p ruscadb-ffi --target x86_64-unknown-linux-gnu
```

`ASAN_OPTIONS=detect_leaks=0` porque el allocator de Rust no es leak-clean por
diseño; interesan los errores de memoria. El job `sanitizers` es **alert-only**
(`continue-on-error: true`) y sube `asan-test.log` como artifact.

### Loom — model checking del MVCC (SPEC-0053)

El job `loom` de nightly explora planificaciones de commits/snapshots
concurrentes con `RUSTFLAGS="--cfg loom"` (sin toolchain nightly):

```bash
RUSTFLAGS="--cfg loom" cargo test -p ruscadb-txn --test loom_mvcc
```

Alert-only (`continue-on-error: true`). Sin el flag, loom ejecuta cada modelo
una sola vez (sigue verde); con el flag explora las planificaciones acotadas
(`preemption_bound`).

### macOS — cross-platform fuera del PR (SPEC-0046)

La cobertura macOS se movió fuera del PR: el job `test-macos` de `nightly.yml`
corre en `macos-latest` con `continue-on-error: true` (alert-only):

```bash
cargo test --workspace --all-features --locked
```

Se ejecuta en `schedule`/`workflow_dispatch` porque la cola de runners macOS es
larga. `scripts/check_ci_config.py` valida que nightly declare un job en
`macos-latest` y que `ci.yml` mantenga la matriz rápida ubuntu+windows.

### Benches — criterion smoke P95 (SPEC-0059)

El job `benches` de nightly compila y corre los benches criterion en modo
smoke (`-- --test`: una pasada por bench, sin medición estadística), el
equivalente al AC-0059-01:

```bash
# Comando real del job `benches` de nightly.yml (ejecuta el AC como test #[ignore]):
cargo test -p ruscadb --test bench_smoke -- --ignored --exact test_ac_0059_01_benches_compile_and_run

# Smoke local (< 5 min, NF-0059-01) — misma cobertura que el AC:
cargo bench -p ruscadb --bench facade_smoke \
  -p ruscadb-wal --bench wal_commit \
  -p ruscadb-vector --bench knn_latency -- --test

# Medición real con baseline (comparación entre ramas):
cargo bench --workspace --benches -- --save-baseline main
cargo bench --workspace --benches -- --baseline main
```

Los benches viven en `crates/{ruscadb,ruscadb-wal,ruscadb-vector}/benches/`
(point-read indexado + ingest blob 1 MiB, group-commit WAL c=64, latencia
HNSW k=10). La baseline versionada inicial está en
`benches/baselines/initial.md` (tablas p50/p95 + receta `critcmp`). El gate
P95 ≤ +5 % alert-only se valida comparando contra esa baseline; el job CI es
`continue-on-error: true` y el AC vive como test `#[ignore]` en
`crates/ruscadb/tests/bench_smoke.rs` para no penalizar el T1.

### SBOM (CycloneDX, supply chain §6.5)

Job `sbom` de T1 (informativo): `cargo cyclonedx --format json --all`.

---

## Nota: toolchain de cargo-mutants

`.cargo/mutants.toml` **no** admite una clave `toolchain` (cargo-mutants usa
`deny_unknown_fields`). El pin de reproducibilidad vive en `rust-toolchain.toml`
(canal estable 1.97.1) y la selección explícita se hace en la **invocación**
(`cargo +<toolchain> mutants`). `scripts/check_ci_config.py` y el test
`test_ac_0016_02_mutants_config_is_valid` verifican esta política para no romper
el parseo de T1/T2/T3.

---

## Cómo se valida esta configuración

| Artefacto | Gate | Comando |
|---|---|---|
| `ci.yml` (matriz rápida) | SPEC-0033/SPEC-0046/AC-01 | `python scripts/check_ci_config.py`, `cargo test -p xtask` |
| `nightly.yml` (`test-macos`) | SPEC-0046/AC-02 | idem |
| `ci.yml` (`mutation-diff`) | SPEC-0061/T2m | `scripts/mutation_diff_gate.py` (MS_diff ≥ 70 %) |
| `ci.yml` (`coverage`) | SPEC-0061 | `scripts/coverage_gate.py` (core ≥ 80/70) |
| `ci.yml` (`commits`) | SPEC-0089/AC-03 | `scripts/check_commits.py` + `scripts/check_changelog.py` |
| `release.yml` (tags `v*`) | SPEC-0089/AC-03 | `scripts/check_changelog.py --tag` + SBOM |
| `ci.yml` (`msrv`) | SPEC-0090/AC-01 | `scripts/check_ci_config.py` (`rust-version`) |
| `ruscadb` (`Database::update`) | SPEC-0091/AC-01 | `cargo test -p ruscadb --test row_update_api` |
| `ruscadb-mcp` (`update`/`delete`) | SPEC-0091/AC-02..04 | `cargo test -p ruscadb-mcp --test mcp_write_tools_advanced` |
| `ci.yml` (`coverage`, BRDA) | SPEC-0092/AC-02 | `scripts/branch_coverage_report.py` + `check_ci_config.py` |
| `ruscadb` (`<data>.cache.json`) | SPEC-0093/AC-01..04 | `cargo test -p ruscadb --test semantic_cache_persistence` |
| `ci.yml` (`red-first`) | SPEC-0094/AC-02 | `cargo xtask red` + `scripts/red_first_gate.py` |
| `nightly.yml` (`benches`) | SPEC-0059 | `bench_smoke` (AC-0059-01, `#[ignore]`) |
| `nightly.yml` (`loom`) | SPEC-0053 | `RUSTFLAGS="--cfg loom"` |
| `xtask contract` | SPEC-0061 | `cargo xtask contract` (marcadores + estados) |
| `check_ci_config.py` (runners) | SPEC-0046/AC-03 | idem |
| `nightly.yml` (sanitizers) | SPEC-0033/AC-02 | idem |
| `nightly.yml` | SPEC-0016/AC-01 | idem |
| `.cargo/mutants.toml` | SPEC-0016/AC-02 | idem |
| `deny.toml` | SPEC-0016/AC-03 | idem |
| `.pre-commit-config.yaml` | §6.5 | `pre-commit run --all-files` |
