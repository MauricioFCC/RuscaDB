---
id: SPEC-0200
feature: hash_join
status: implemented
maturity: real
owner: query-team
appetite_days: 2
boundaries:
  crates: [ruscadb]
  files:
    - crates/ruscadb/src/join.rs
    - crates/ruscadb/tests/join_hash.rs
  out_of_scope: [join de más de dos tablas, join no-equi, reordering]
fr:
  - { id: FR-0200-01, desc: "join de igualdad sin indice interior usa hash join O(n+m) en vez de nested loop O(n*m)" }
  - { id: FR-0200-02, desc: "la semantica de INNER/LEFT/RIGHT/FULL y NULL se conserva" }
nf:
  - { id: NF-0200-01, desc: "sin dependencias nuevas; HashMap std; memoria O(n) para el lado interior" }
  - { id: NF-0200-02, desc: "coerción numérica Int/UInt/Float homogénea con keys_equal; NaN y NULL nunca emparejan" }
acceptance_criteria:
  - id: AC-0200-01
    given: tablas sin indice en la columna de igualdad y filas NULL
    when: se ejecutan INNER/LEFT/RIGHT/FULL JOIN
    then: el resultado coincide con el oraculo y NULL no empareja
    test: test_ac_0200_01_hash_join_outer_oracles
  - id: AC-0200-02
    given: dos tablas de 10 000 filas sin indice
    when: se ejecuta un join de igualdad
    then: devuelve 10 000 parejas muy por debajo de 5 s (no O(n*m))
    test: test_ac_0200_02_hash_join_scales_linearly
exit_criteria:
  - cargo test -p ruscadb --test join_hash
  - cargo test -p ruscadb --all-features
  - cargo xtask trace
  - cargo xtask contract
  - cargo xtask audit
rollback:
  - revertir el commit; el nested loop anterior sigue disponible en el historial
sandbox:
  - cargo test -p ruscadb --test join_hash -- --nocapture
---
# SPEC-0200 — Hash join para igualdad sin índice

## Contexto

`execute_join_at` usa `index_lookup_eq` cuando la columna interior tiene índice.
Si no lo tiene, recorre el producto cartesiano (`O(n·m)`); en tablas medianas
(10k × 10k) esto bloquea el motor. La madurez lo listaba como "join no-equi
O(n·m)" pero el caso **equi sin índice** también lo sufría.

## Diseño

1. Construir `HashMap<JoinKey, Vec<&Record>>` con las filas interiores visibles.
2. Sondear una vez por fila exterior; conservar el orden de filas interior
   dentro de cada bucket para que la salida sea determinista.
3. `JoinKey` normaliza `Int`/`UInt`/`Float` a bits IEEE-754 (misma coerción que
   `keys_equal`), `-0.0 → 0.0`; `NaN`, `NULL`, `Bytes` y `Timestamp` no
   generan clave (nunca emparejan).
4. Mantener la ruta indexada (más barata para claves selectivas).

## Complejidad

```text
antes:  O(n·m)   nested loop sin índice
después: O(n + m + k) esperado   (k = parejas)
memoria: O(m) para el hash del lado interior
```

## TDD adversarial

- Oráculo congelado: INNER/LEFT/RIGHT/FULL sobre tablas pequeñas con NULL.
- Coerción numérica: el mismo `JoinKey` para `Int(1)`, `UInt(1)` y `Float(1.0)`.
- Escalado: 10k × 10k debe terminar en < 5 s (el nested loop no lo cumple).

## Estado de implementación (2026-11-03)

- Rama `else` de `execute_join_at` sustituida por hash join; ruta indexada
  intacta. Tests AC-0200-01/02 en `crates/ruscadb/tests/join_hash.rs`.
