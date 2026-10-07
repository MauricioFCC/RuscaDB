<!-- Gracias por contribuir a RuscaDB. Rellena lo aplicable y borra lo demás. -->

## Qué y por qué

<!-- Qué cambia y por qué. Enlaza el issue/spec si existe. -->

## Spec y trazabilidad

- [ ] Existe `specs/<feature>.md` (nueva o actualizada) con criterios de aceptación.
- [ ] Cada AC nuevo tiene su test `test_ac_XXXX_NN_*` (lo exige `cargo xtask trace`).

## Checklist (gate T1)

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`
- [ ] `python scripts/check_architecture.py`
- [ ] `python scripts/check_core_unsafe.py`
- [ ] `cargo xtask trace`
- [ ] Mutation score ≥ 70% en los crates tocados

## Notas para el revisor

<!-- Decisiones de diseño, tradeoffs, riesgos, rollback. -->

## Evidencia

<!-- Pega el output crudo de los gates (test result, mutation score). -->
