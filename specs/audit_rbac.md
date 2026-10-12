---
id: SPEC-0066
feature: audit_rbac
status: implemented
owner: core-team
appetite_days: 5
boundaries:
  crates: [ruscadb, ruscadb-core]
  out_of_scope: [persistencia del audit en disco, usuarios/contraseñas, roles por sesion remota]
fr:
  - { id: FR-0066-01, desc: "RBAC por rol de instancia (Read/Write/Admin, default Admin): Read no escribe; Write no administra; Admin todo" }
  - { id: FR-0066-02, desc: "audit en memoria de operaciones (execute/insert/delete/put_blob con accion y timestamp en nanos), consultable y reseteable" }
nf:
  - { id: NF-0066-01, desc: "cero regresion: Admin por defecto; con role Admin todas las operaciones actuales funcionan identicas" }
  - { id: NF-0066-02, desc: "denegaciones con error tipado accionable (PermissionDenied WHAT+WHY+WHERE), sin panics" }
acceptance_criteria:
  - id: AC-0066-01
    given: role Read
    when: se intenta insert/UPDATE/DELETE o put_blob
    then: RuscaError::PermissionDenied accionable y sin efectos en datos
    test: test_ac_0066_01_read_role_cannot_write
  - id: AC-0066-02
    given: role Write
    when: se intenta register_model, set_role, clear_audit o gc_blobs
    then: PermissionDenied (operaciones de administracion reservadas a Admin)
    test: test_ac_0066_02_write_role_cannot_administer
  - id: AC-0066-03
    given: role Admin (por defecto)
    when: se ejecuta una bateria completa (insert, SELECT, KNN, put_blob)
    then: todo funciona como hoy (cero regresion)
    test: test_ac_0066_03_admin_role_full_access
  - id: AC-0066-04
    given: audit habilitado
    when: se ejecutan insert, execute y put_blob
    then: audit_trail contiene 3+ entradas en orden con accion y nanos > 0
    test: test_ac_0066_04_audit_records_operations
  - id: AC-0066-05
    given: audit OFF (por defecto) o tras clear_audit
    when: se ejecuta la misma bateria
    then: audit_trail vacio (cero overhead de captura)
    test: test_ac_0066_05_audit_off_by_default_and_clearable
exit_criteria:
  - cargo test -p ruscadb --test audit_rbac -- test_ac_0066
  - cargo test -p ruscadb  (suite previa verde)
  - cargo clippy -p ruscadb --all-targets -- -D warnings
  - cargo mutants -p ruscadb sin --in-place, MS del diff >= 70%
rollback:
  - eliminar Role/AuditEntry y sus checks; execute vuelve al camino directo
sandbox:
  - cargo test -p ruscadb --test audit_rbac
---

# SPEC-0066 — Audit + RBAC

## Contexto

Línea futura del roadmap: **audit + RBAC**. RuscaDB es embebida y
single-process; el RBAC es por **rol de instancia** (el host elige el rol al
configurar), no por usuarios/red. El audit es un registro en memoria de
operaciones con timestamp — suficiente para diagnóstico y forense post-mortem
(su persistencia queda fuera de alcance). Se añade `RuscaError::PermissionDenied`
al dominio.

## Superficie de API

```rust
pub enum Role { Read, Write, Admin }   // default: Admin
pub struct AuditEntry { pub action: String, pub detail: String, pub nanos: u64 }
impl Database {
    pub fn set_role(&mut self, role: Role);
    pub fn current_role(&self) -> Role;
    pub fn set_audit(&mut self, enabled: bool);
    pub fn audit_trail(&self) -> Vec<AuditEntry>;
    pub fn clear_audit(&mut self);
}
```

Reglas: `Read`: bloquear insert, insert_record, update/delete (DML en execute),
put_blob. `Write`: bloquear register_model, set_role, set_audit, clear_audit,
gc_blobs. `Admin`: todo. (Los setters de configuración con firma `()` —p. ej.
`set_semantic_cache`, `reset_cost_report`— no pueden denegar con error tipado y
quedan fuera del RBAC en esta iteración.) Audit ON:
`execute`, `insert*`, `delete`, `put_blob` emiten `AuditEntry` (acción, detalle,
`nanos`).

## Criterios de aceptación

- **AC-0066-01/02/03** — Permisos por rol con error tipado.
- **AC-0066-04/05** — Audit en memoria consultable y OFF por defecto.

## Trazabilidad

Tests `test_ac_0066_<nn>_*`; verificado por `cargo xtask trace`/`contract`.