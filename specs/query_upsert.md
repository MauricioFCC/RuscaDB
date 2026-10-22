---
id: SPEC-0107
feature: query_upsert
status: implemented
owner: query-team
appetite_days: 4
boundaries:
  crates: [ruscadb, ruscadb-query]
  files: [crates/ruscadb/src/dml.rs, crates/ruscadb/src/executor.rs, crates/ruscadb-query/src/ast.rs, crates/ruscadb-query/src/parser.rs, crates/ruscadb/tests/query_upsert.rs]
  out_of_scope: [MERGE SQL:2003, constraints UNIQUE (no existen en RuscaDB; la ambiguedad se resuelve en runtime), ON CONFLICT arbitrario (solo igualdad por columnas), upsert sobre tablas con indices derivados HNSW/CSR/FTS (solo heap), triggers, RETURNING de columnas no confirmadas, upsert sobre varias tablas]
fr:
  - { id: FR-0107-01, desc: "el parser acepta `INSERT INTO t (cols) VALUES (...) ON CONFLICT (cols) DO NOTHING | DO UPDATE SET col = literal | excluded.col` sin keywords nuevos (ON/CONFLICT/DO/NOTHING/UPDATE/SET/excluded son identificadores validados contextualmente)" }
  - { id: FR-0107-02, desc: "ejecucion por fila propuesta: 0 coincidentes => inserta; 1 coincidente => aplica la accion; >1 coincidentes => error atomico de ambiguedad (sin commit parcial)" }
  - { id: FR-0107-03, desc: "DO NOTHING deja la fila existente intacta y no la reporta como actualizada" }
  - { id: FR-0107-04, desc: "DO UPDATE SET col = excluded.col toma el valor de la fila propuesta; col = literal toma el literal; las columnas no mencionadas se conservan" }
  - { id: FR-0107-05, desc: "RETURNING devuelve las filas efectivamente insertadas/actualizadas con sus valores finales, en el orden de la propuesta" }
  - { id: FR-0107-06, desc: "multi-row upsert es atomico: un solo commit, o rollback completo si falla alguna fila" }
nf:
  - { id: NF-0107-01, desc: "la busqueda de coincidencia usa el indice primario si existe (O(log n)), si no full scan" }
  - { id: NF-0107-02, desc: "RETURNING preserva el orden de la propuesta" }
  - { id: NF-0107-03, desc: "el error de ambiguedad no deja filas parcialmente aplicadas (transaccion)" }
acceptance_criteria:
  - id: AC-0107-01
    given: una tabla sin filas que coincidan con la propuesta
    when: se ejecuta INSERT ... ON CONFLICT (...) DO UPDATE SET ...
    then: inserta la fila propuesta (nueva fila) y RETURNING la reporta
    test: test_ac_0107_01_upsert_inserts_when_no_conflict
  - id: AC-0107-02
    given: una fila existente que coincide por las columnas de conflicto
    when: se ejecuta el upsert con DO UPDATE
    then: actualiza la fila existente con los valores de SET y RETURNING reporta la fila final
    test: test_ac_0107_02_upsert_updates_single_match
  - id: AC-0107-03
    given: DO UPDATE SET col = excluded.col
    when: se ejecuta el upsert
    then: el valor actualizado es el de la fila propuesta (excluded), no el literal ni el previo
    test: test_ac_0107_03_upsert_excluded_resolves_from_proposed_row
  - id: AC-0107-04
    given: una fila existente que coincide y la accion es DO NOTHING
    when: se ejecuta el upsert
    then: la fila existente queda intacta (valores previos) y no se reporta como actualizada
    test: test_ac_0107_04_do_nothing_leaves_row_untouched
  - id: AC-0107-05
    given: dos o mas filas existentes que coinciden por las columnas de conflicto
    when: se ejecuta el upsert
    then: falla con error atomico de ambiguedad (TypeMismatch/Conflict) y ninguna fila queda aplicada
    test: test_ac_0107_05_ambiguous_target_fails_atomically
  - id: AC-0107-06
    given: varias filas propuestas en un solo INSERT
    when: se ejecuta el upsert
    then: todas se aplican en un solo commit (o se revierte todo si una falla)
    test: test_ac_0107_06_multi_row_upsert_single_commit
  - id: AC-0107-07
    given: varias filas propuestas con mezcla de insert y update
    when: se ejecuta el upsert
    then: cada fila se resuelve independientemente (insert si no coincide, update si coincide) en un solo commit
    test: test_ac_0107_07_multi_row_mixed_actions
  - id: AC-0107-08
    given: un upsert con RETURNING
    when: se ejecuta
    then: RETURNING lista las filas efectivamente insertadas o actualizadas (valores finales) en orden de propuesta
    test: test_ac_0107_08_returning_reports_effective_rows
  - id: AC-0107-09
    given: un upsert con DO UPDATE que menciona solo algunas columnas
    when: se ejecuta
    then: las columnas no mencionadas conservan su valor previo (update parcial)
    test: test_ac_0107_09_upsert_preserves_unmentioned_columns
exit_criteria:
  - cargo test -p ruscadb --all-features --test query_upsert
  - cargo test -p ruscadb-query --all-features
  - cargo xtask red specs/query_upsert.md (evidencia test-first)
  - cargo fmt --all -- --check
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo test --workspace --all-features
  - cargo doc --workspace --no-deps --all-features (RUSTDOCFLAGS=-D warnings)
  - python scripts/check_architecture.py
  - python scripts/check_core_unsafe.py
  - python scripts/check_ci_config.py
  - cargo xtask trace / contract verdes
  - cargo mutants --in-diff (-p ruscadb y -p ruscadb-query) MS_diff >= 70%
rollback:
  - revertir dml.rs, executor.rs, ast.rs, parser.rs y tests al commit previo;
    el INSERT sin ON CONFLICT sigue funcionando (regresion cero)
sandbox:
  - cargo test -p ruscadb --all-features --test query_upsert
---

# SPEC-0107 — UPSERT (`INSERT ... ON CONFLICT`)

## Contexto

RuscaDB soporta `INSERT` y `UPDATE` por separado (SPEC-0012/0091) pero no
la forma idiomática de "insertar o actualizar", que es la operación que más
se repite en cargas de datos y en sincronización con sistemas externos. Hoy
el usuario debe hacer `SELECT` + `INSERT`/`UPDATE` en dos pasos, con carrera
entre ellos y sin atomicidad garantizada.

El SQL estándar (SQL:2003 `MERGE`) y las extensiones modernas
(PostgreSQL 9.5 `INSERT ... ON CONFLICT`, SQLite 3.24 `UPSERT`) resuelven
esto en una sola sentencia. RuscaDB adopta la forma de PostgreSQL/SQLite
(`ON CONFLICT`) por ser la más compacta y la que ya encaja con el parser
actual (`INSERT` ya existe); `MERGE` queda fuera de alcance por su gramática
de `WHEN MATCHED/NOT MATCHED`.

## Superficie

- `crates/ruscadb-query/src/ast.rs` — `OnConflict { target: Vec<String>,
  action: ConflictAction }`, `ConflictAction::{DoNothing, DoUpdate(Vec<Assignment>)}`,
  `Assignment { column, value: ConflictValue }`, `ConflictValue::{Literal(Expr), Excluded(String)}`.
- `crates/ruscadb-query/src/parser.rs` — `parse_on_conflict` tras `VALUES (...)`,
  validando identificadores y la forma `excluded.col`.
- `crates/ruscadb/src/dml.rs` — despacho del upsert por fila (busqueda de
  coincidencia, accion, RETURNING) dentro de la transaccion existente.
- `crates/ruscadb/src/executor.rs` — solo si hace falta un helper de
  evaluacion reutilizable (idealmente el upsert vive en `dml.rs`).
- `crates/ruscadb/tests/query_upsert.rs` — AC-0107-01..09 (API publica).

Queda fuera: `MERGE` SQL:2003, constraints `UNIQUE` (RuscaDB no las tiene;
la ambiguedad se resuelve en runtime con error atomico cuando hay >1
coincidente), `ON CONFLICT` arbitrario (solo igualdad por columnas),
upsert sobre tablas con indices derivados (HNSW/CSR/FTS — solo heap),
triggers, `RETURNING` de columnas no confirmadas y upsert sobre varias
tablas.

## Estado del arte (FRS, verificado web)

- **PostgreSQL 9.5+ `INSERT ... ON CONFLICT`** (postgresql.org/docs/current/sql-insert.html):
  `ON CONFLICT (conflict_target) DO NOTHING | DO UPDATE SET column = { expression | excluded.column | DEFAULT }`.
  La pseudo-tabla `excluded` referencia la fila propuesta. RuscaDB adopta la
  misma sintaxis y semantica, salvo que el conflicto se detecta por igualdad
  sobre las columnas nombradas (no hay constraint UNIQUE que dispararlo).
- **SQLite 3.24+ `UPSERT`**: misma forma `ON CONFLICT (cols) DO UPDATE SET ...`,
  con `excluded.col`. RuscaDB reutiliza el vocabulario (`excluded`).
- **Ambiguedad sin constraint**: como RuscaDB no tiene UNIQUE, una "conflicto"
  puede tener N coincidentes. La decision de diseño (NF-0107-03) es fallar
  en voz alta con error atomico cuando N > 1, en vez de elegir una fila
  arbitraria: predecible y auditable, coherente con la doctrina ERR.
- **Atomicidad**: todo el upsert (multi-row) es una sola transaccion; un
  fallo en una fila revierte las anteriores (NF-0107-03), igual que el resto
  de DML multi-row de RuscaDB.

## Notas de diseño

- **Sin keywords nuevos.** `ON`, `CONFLICT`, `DO`, `NOTHING`, `UPDATE`, `SET`
  y `excluded` se validan como identificadores en contexto (el lexer ya los
  trata como `Ident` cuando no son palabras reservadas del dialecto RuscaDB;
  si alguno fuera keyword reservado, se acepta via `match_keyword` en modo
  no-reservado). Esto mantiene el parser simple y evita romper consultas
  existentes que usen esos nombres como columnas.
- **AST mínimo.** `Insert` gana un campo opcional `on_conflict: Option<OnConflict>`
  (None = INSERT puro, comportamiento actual intacto — regresion cero).
- **Resolución de `excluded.col`.** En `DO UPDATE SET col = excluded.col`, el
  valor se toma de la fila propuesta (la que se intentaba insertar), no de la
  existente ni de un literal. Se valida que `col` exista en el esquema de la
  tabla.
- **Detección de conflicto.** Para cada fila propuesta, buscar filas
  existentes con igualdad en TODAS las columnas del target (AND). Índice
  primario si el target es la PK (O(log n)); si no, full scan (NF-0107-01).
  Si hay 0 → insert; 1 → update; >1 → error atomico.
- **Acción sobre update.** Aplicar los `SET` sobre la fila existente (update
  parcial: solo las columnas mencionadas cambian; el resto se conserva).
  `RETURNING` reporta la fila final (insertada o actualizada).
- **Atomicidad.** Todo el upsert corre dentro de una transaccion; si una fila
  falla (ambiguedad, tipo incompatible, etc.) se revierte todo (NF-0107-03).
- **Sin cambios en el planner.** El upsert no usa `IndexScan`/`FullScan`
  del SELECT; la busqueda de conflicto es directa sobre el heap.
- **Supersesion.** `docs/MVP.md` bullet "RQL acotado" menciona
  "DML sin `UPSERT`/`MERGE`" — al cerrar 0107 se retira esa clausula del
  bullet (igual que 0105/0106 retiraron sus limites).
