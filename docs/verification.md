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

# 5) Config de CI/supply chain (SPEC-0016)
python scripts/check_ci_config.py

# 6) Supply chain
cargo deny check          # advisories + licenses + bans + sources (deny.toml)
cargo audit               # RustSec advisories
```

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

### macOS — cross-platform fuera del PR (SPEC-0046)

La cobertura macOS se movió fuera del PR: el job `test-macos` de `nightly.yml`
corre en `macos-latest` con `continue-on-error: true` (alert-only):

```bash
cargo test --workspace --all-features --locked
```

Se ejecuta en `schedule`/`workflow_dispatch` porque la cola de runners macOS es
larga. `scripts/check_ci_config.py` valida que nightly declare un job en
`macos-latest` y que `ci.yml` mantenga la matriz rápida ubuntu+windows.

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
| `check_ci_config.py` (runners) | SPEC-0046/AC-03 | idem |
| `nightly.yml` (sanitizers) | SPEC-0033/AC-02 | idem |
| `nightly.yml` | SPEC-0016/AC-01 | idem |
| `.cargo/mutants.toml` | SPEC-0016/AC-02 | idem |
| `deny.toml` | SPEC-0016/AC-03 | idem |
| `.pre-commit-config.yaml` | §6.5 | `pre-commit run --all-files` |
