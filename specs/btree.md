---
id: SPEC-0020
feature: btree
status: accepted
owner: storage-team
appetite_days: 8
boundaries:
  crates: [ruscadb-btree, ruscadb-core]
  out_of_scope: [persistencia en paginas, integracion en la fachada, concurrencia]
fr:
  - { id: FR-0020-01, desc: "B+tree ordenado generico BPlusTree<K: Ord, V: Clone>" }
  - { id: FR-0020-02, desc: "insert/get/remove con rebalanceo por split y merge de hojas/nodos" }
  - { id: FR-0020-03, desc: "range(start..end) devuelve pares ordenados por clave (scan de hojas enlazadas)" }
  - { id: FR-0020-04, desc: "invariantes de nodo: ocupacion minima/maxima (orden del arbol configurable)" }
  - { id: FR-0020-05, desc: "len/is_empty y errores accionables ante claves invalidas" }
nf:
  - { id: NF-0020-01, desc: "get/insert O(log n) en el peor caso (altura logaritmica)" }
  - { id: NF-0020-02, desc: "sin panics; roundtrip insert->get->remove exacto (proptest)" }
acceptance_criteria:
  - id: AC-0020-01
    given: un arbol vacio
    when: se insertan y consultan claves
    then: get devuelve el valor correcto y sobrescribe sin duplicar
    test: test_ac_0020_01_insert_get_overwrite
  - id: AC-0020-02
    given: un arbol con claves
    when: se eliminan claves
    then: remove funciona y las claves ausentes no alteran el arbol
    test: test_ac_0020_02_remove_and_missing
  - id: AC-0020-03
    given: un arbol con claves dispersas
    when: se hace range
    then: devuelve los pares dentro del rango en orden ascendente
    test: test_ac_0020_03_range_is_sorted_and_bounded
  - id: AC-0020-04
    given: muchas claves en orden aleatorio (fuerza splits y merges)
    when: se consultan todas
    then: todas se recuperan y len coincide (integracion estructural)
    test: test_ac_0020_04_many_keys_survive_splits
  - id: AC-0020-05
    given: un arbol
    when: se eliminan claves hasta vaciarlo
    then: len=0, is_empty=true y el arbol sigue operativo
    test: test_ac_0020_05_delete_to_empty_rebalances
exit_criteria:
  - cargo test -p ruscadb-btree -- test_ac_0020
  - cargo mutants -p ruscadb-btree mutation score >= 70%
  - T2 spec_fidelity >= 0.90
rollback:
  - eliminar el crate ruscadb-btree del workspace
sandbox:
  - cargo test -p ruscadb-btree
---

# SPEC-0020 — B+tree ordenado (índice primario/secundario)

## Contexto

El roadmap (§5.4) pide un **B+tree** para el índice relacional y ADR-003 sitúa los
metadatos/grafo/FTS en estructuras ordenadas nativas. Se implementa un B+tree
genérico en un crate aislado `ruscadb-btree`:

- `BPlusTree<K: Ord, V: Clone>` con `order` (máximo de hijos) configurable.
- Nodos internos con separadores; hojas enlazadas (`next`/`prev`) para `range`.
- `insert` con *split* aguas arriba; `remove` con *borrow*/*merge*.
- `get`, `contains_key`, `range(a..b)`, `len`, `is_empty`.

La persistencia en páginas y la integración en la fachada (sustituir el índice
ordenado en memoria de SPEC-0012) quedan para una iteración posterior.

## Criterios de aceptación

- **AC-0020-01..02** — insert/get/overwrite/remove.
- **AC-0020-03** — `range` ordenado y acotado.
- **AC-0020-04** — splits/merges con muchas claves.
- **AC-0020-05** — vaciado con rebalanceo.

## Trazabilidad

Tests `test_ac_0020_<nn>_*` + proptests (equivalencia con `BTreeMap`); verificado
por `cargo xtask trace`.
