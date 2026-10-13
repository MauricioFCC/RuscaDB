---
id: SPEC-XXXX
feature: <nombre_de_la_feature>
status: proposed
owner: <equipo>
appetite_days: <días>
boundaries:
  crates: [<crates tocadas>]
  files: [<ficheros clave>]
  out_of_scope: [qué NO toca esta spec]
fr:
  - { id: FR-XXXX-NN, desc: "<requisito funcional 1>" }
  - { id: FR-XXXX-NN, desc: "<requisito funcional 2>" }
nf:
  - { id: NF-XXXX-NN, desc: "<requisito no funcional 1>" }
acceptance_criteria:
  - id: AC-XXXX-NN
    given: <estado inicial>
    when: <acción bajo prueba>
    then: <resultado observable y medible>
    test: <test_name_ac_XXXX_NN>
exit_criteria:
  - <comando de verificación 1>
  - cargo clippy --workspace --all-targets --all-features -- -D warnings
  - cargo xtask trace / contract verdes
rollback:
  - <cómo se revierte sin dejar estado huérfano>
sandbox:
  - <comando de prueba aislada>
---

# SPEC-XXXX — <título>

## Contexto

<por qué existe esta spec: gap, research/fuente de frontera, qué resuelve>

## Superficie

<qué toca: crates, ficheros, API pública; qué queda explícitamente fuera>

## Notas de diseño

<decisiones, alternativas descartadas y enlaces a research>
