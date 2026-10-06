El documento que has compartido es un **plano maestro de nivel excepcional**. No le falta diseño, ni arquitectura, ni visión. Lo que "falta" ahora es **la ejecución rigurosa** para transformar este texto en código, siguiendo estrictamente la metodología SDAD que tú mismo definiste en el §2.

Para pasar de este documento a la primera línea de código productiva, aquí tienes el desglose exacto de lo que falta y los pasos inmediatos que debes dar:

---

### 1. Los "Detalles Finos" Técnicos (Pre-código)
Aunque la arquitectura está clara, hay definiciones de bajo nivel que necesitas concretar antes de que los agentes o tú puedan sintetizar código:

*   **La Sintaxis Exacta del Dialecto (Query Language):** El §4.6 (ADR-007) menciona extensiones tipo SurrealQL (`->`, `@>`, `TRAVERSE`, `KNN`). Falta definir la **gramática formal** (usando `pest` o `lalrpop` en Rust) para que el parser de DataFusion pueda extenderse.
*   **Definición Total de Tipos en `ruscadb-core`:** En el §5.5 defines los *Traits* (`StorageEngine`, `TxnManager`, etc.), pero falta definir al 100% los tipos de datos puros: `RuscaError` (con sus variantes exactas), `RecordId` (ULID), `ScalarMap`, `BlobPointer`, `EdgeSet`.
*   **El Pipeline del "Juez LLM" (Gate T2):** El §2.1 y §7.1 mencionan un "LLM-judge" para validar la *Spec Fidelity*. Falta diseñar el *prompt system* y el script de orquestación que tomará el diff de código, la spec en YAML y emitirá el veredicto de 0 a 1.

### 2. El "Día 1": Fase F0 (Fundación)
No empieces programando la base de datos. Empieza programando el **entorno de trabajo**. Según tu roadmap, la Fase F0 (Semanas 1-2) debe entregar lo siguiente:

1.  **Estructura del Workspace Cargo:** Crea el `Cargo.toml` raíz con `resolver = "2"` y genera los 12 crates definidos en el §4.3.
2.  **Configuración de Lints y Seguridad:**
    *   Crea el `deny.toml` y `rust-toolchain.toml`.
    *   Aplica `#![forbid(unsafe_code)]` en `ruscadb-core`.
    *   Configura `cargo-mutants` y `cargo-geiger` en el CI.
3.  **El Primer Artefacto SDD:** Escribe la primera spec machine-readable en `specs/core_record.md` o `specs/wal_durability.md`.
4.  **CI Pipeline (T1):** Configura GitHub Actions para que el pipeline T1 (el de <90s) corra en cada PR.

### 3. La Fase F1: El Núcleo Duro (Storage & WAL)
Una vez el entorno esté listo, debes atacar el corazón de RuscaDB. No toques los bindings (Python/Node) ni la IA todavía. Sigue el ciclo **Spec → Test (RED) → Code → Refactor**:

*   **Paso 3.1: `ruscadb-core`**
    *   Define el `Record` unificado (§4.4) y el catálogo.
    *   *Test:* Asegurar que un `Record` se serializa/deserializa sin pérdida de información (Invariantes I6, I9).
*   **Paso 3.2: `ruscadb-wal` (El más crítico)**
    *   Implementa el frame WAL: `[ len u32 | lsn u64 | tx_id u64 | kind u8 | payload | crc32c u32 ]`.
    *   Implementa el recovery truncando en el primer CRC inválido.
    *   *Test Adversarial:* Inyectar `SIGKILL` a mitad de escritura y verificar que al reabrir no hay corrupción (AC-0012-01).
*   **Paso 3.3: `ruscadb-storage`**
    *   Integra Apache Lance/Arrow.
    *   Implementa el Buffer Pool con presupuesto duro (LRU-K).

### 4. Infraestructura y "Territorio Digital"
Si el proyecto va en serio, necesitas asegurar la identidad de **RuscaDB** hoy mismo antes de que alguien más lo haga:

1.  **Registros de Paquetes:**
    *   Publica un crate vacío (o con solo el README) en `crates.io` con el nombre `ruscadb` (o `rusca-db`).
    *   Reserva `ruscadb` en PyPI (Python) y npm (Node.js).
2.  **Repositorio y Dominio:**
    *   Crea la organización en GitHub (`ruscadb`).
    *   Registra el dominio `ruscadb.dev`, `ruscadb.io` o `rusca.rs`.
3.  **Identidad Visual:**
    *   Diseña un logo minimalista (una cucaracha geométrica/estilizada como un chip o un nodo de grafo) para el README.

### 5. ¿Cómo empezar a programar HOY mismo?
Si vas a usar agentes de IA (como sugiere tu metodología SDAD en el §2.1), tu primer *prompt* o *intent capture* debe ser exactamente este:

> **Intent:** Inicializar el workspace de RuscaDB (Fase F0).
> **Spec:** Crear la estructura de 12 crates en Rust, configurar `cargo-deny`, `cargo-mutants`, y establecer el `#![forbid(unsafe_code)]` en el crate `core`.
> **Failing Test:** Un script de CI que falle si el crate `core` contiene código `unsafe` o si el grafo de dependencias tiene ciclos.

**Resumen:**
El documento está perfecto. Lo que falta es **disciplina**. El mayor riesgo de RuscaDB no es técnico, es saltarse la metodología SDAD y empezar a "hackear" código sin pasar por el gate de las specs y los tests adversariales.

Si sigues el §2.1 al pie de la letra, en 6 meses tendrás el MVP más robusto y seguro del mercado de bases de datos embebidas. ¿Quieres que redactemos la primera `specs/<feature>.md` machine-readable para arrancar la Fase F1?
