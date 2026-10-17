//! Cifrado en reposo de la fachada RuscaDB (SPEC-0013, FR-0013-01).
//!
//! [`EncryptionConfig`] guarda la clave simétrica de 32 B que [`crate::DbConfig`]
//! propaga al WAL: construcción directa con [`EncryptionConfig::new`] o derivada
//! de una passphrase con Argon2id vía [`EncryptionConfig::from_passphrase`]. El
//! material de clave vive en [`zeroize::Zeroizing`] y su [`std::fmt::Debug`] está
//! redactado (nunca imprime la clave).
//!
//! ## Límites documentados
//!
//! - El heap `.data` hereda el cifrado vía WAL: en modo cifrado `commit()` no
//!   publica páginas al `.data` (el WAL es la fuente de verdad) y las páginas
//!   permanecen `dirty` en el pool (nunca se desalojan: un pool lleno falla en
//!   voz alta con `BufferPoolFull`, nunca sirve datos rancios). Dimensiona
//!   `pool_capacity` para el conjunto de trabajo.
//! - Páginas `.data` escritas antes de activar el cifrado siguen en claro hasta
//!   su reescritura; además, abrir con clave un WAL creado en claro (o al revés)
//!   falla en modo estricto con `WalCorrupt`, sin exponer claro.
//! - Sin rotación de claves ni KMS/HSM (fuera de alcance, SPEC-0013).

use ruscadb_core::RuscaError;
use zeroize::Zeroizing;

/// Configuración de cifrado en reposo de una [`crate::Database`].
#[derive(Clone)]
pub struct EncryptionConfig {
    /// Clave simétrica de 32 B (se borra al salir de ámbito).
    key: Zeroizing<[u8; 32]>,
}

impl EncryptionConfig {
    /// Crea la configuración con una clave directa de 32 B.
    ///
    /// Args:
    ///     key: Clave simétrica (se copia a memoria borrable).
    ///
    /// Returns:
    ///     La configuración lista para usar en [`crate::DbConfig`].
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key: Zeroizing::new(key),
        }
    }

    /// Deriva la clave de una passphrase con Argon2id (vía `ruscadb-crypto`).
    ///
    /// Args:
    ///     passphrase: Secreto elegido por el usuario.
    ///     salt: Sal única por clave (se recomiendan >= 8 bytes).
    ///
    /// Returns:
    ///     La configuración con la clave derivada de 32 B.
    ///
    /// Raises:
    ///     [`RuscaError::InvalidConfig`] si Argon2id rechaza la entrada.
    pub fn from_passphrase(passphrase: &str, salt: &[u8]) -> Result<Self, RuscaError> {
        ruscadb_crypto::derive_key(passphrase, salt).map(Self::new)
    }

    /// Presta la clave simétrica (para abrir el WAL cifrado).
    ///
    /// Returns:
    ///     Referencia a los 32 B de clave (siguen perteneciendo al `Zeroizing`).
    pub fn key(&self) -> &[u8; 32] {
        &self.key
    }
}

impl std::fmt::Debug for EncryptionConfig {
    /// Formatea sin exponer la clave (siempre `[redacted]`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionConfig")
            .field("key", &"[redacted]")
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::heap::heap_insert;
    use crate::table_api::maintain_index;
    use crate::{
        ColumnDef, ColumnType, Database, DbConfig, EdgeSet, Record, RecordId, RecordMeta,
        ScalarMap, ScalarValue,
    };
    use proptest::prelude::*;

    /// Clave fija de pruebas (32 B deterministas).
    fn test_key() -> [u8; 32] {
        [0x2Au8; 32]
    }

    /// Marcador en claro que nunca debe aparecer en disco cifrado.
    fn marker_text() -> String {
        "RUSCADB_AC001305_SECRETO_EN_CLARO".to_string()
    }

    /// Configuración de prueba con cifrado activado.
    fn encrypted_config(path: &std::path::Path, key: [u8; 32]) -> DbConfig {
        let mut config = DbConfig::new(path, 64);
        config.encryption = Some(EncryptionConfig::new(key));
        config
    }

    /// `true` si `haystack` contiene `needle` (con guarda de longitud).
    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() || haystack.len() < needle.len() {
            return false;
        }
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// Crea la tabla de secretos y guarda el marcador (con commit).
    fn seed_secret(database: &mut Database, marker: &str) {
        database
            .create_table(
                "secretos",
                vec![ColumnDef {
                    name: "contenido".to_string(),
                    col_type: ColumnType::Text,
                }],
            )
            .expect("create_table");
        let mut scalars = ScalarMap::new();
        scalars.insert(
            "contenido".to_string(),
            ScalarValue::Text(marker.to_string()),
        );
        database.insert("secretos", scalars).expect("insert");
        database.commit().expect("commit");
    }

    /// Lee el marcador con `SELECT *` (falla si falta o difiere).
    fn assert_marker(database: &mut Database, marker: &str) {
        let rows = database.execute("SELECT * FROM secretos").expect("select");
        assert_eq!(rows.len(), 1, "se esperaba una fila tras reopen");
        assert_eq!(
            rows[0].get("contenido").expect("columna contenido"),
            &ScalarValue::Text(marker.to_string())
        );
    }

    /// AC-0013-05 — end-to-end cifrado sin claro en disco.
    #[test]
    // @spec AC-0013-05
    fn test_ac_0013_05_encrypted_database_end_to_end() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("vault.data");
        let wal_path = path.with_extension("wal");
        let marker = marker_text();
        let key = test_key();

        {
            let mut database = Database::open(encrypted_config(&path, key)).expect("open cifrado");
            seed_secret(&mut database, &marker);
            database.close().expect("close");
        }

        let data_bytes = std::fs::read(&path).expect("lee .data");
        let wal_bytes = std::fs::read(&wal_path).expect("lee .wal");
        assert!(
            !wal_bytes.is_empty(),
            "el WAL debe contener frames cifrados"
        );
        assert!(
            !contains_bytes(&data_bytes, marker.as_bytes()),
            "el .data no debe exponer el marcador en claro"
        );
        assert!(
            !contains_bytes(&wal_bytes, marker.as_bytes()),
            "el .wal no debe exponer el marcador en claro"
        );

        {
            let mut database =
                Database::open(encrypted_config(&path, key)).expect("reopen con clave");
            assert_marker(&mut database, &marker);
            database.close().expect("close");
        }

        let reopened = Database::open(DbConfig::new(&path, 64));
        assert!(
            reopened.is_err(),
            "reabrir sin clave un WAL cifrado debe fallar"
        );

        let mut wrong_key = test_key();
        wrong_key[0] ^= 0x01;
        let wrong = Database::open(encrypted_config(&path, wrong_key));
        assert!(
            wrong.is_err(),
            "reabrir con clave errónea debe fallar (AEAD)"
        );

        let mut database =
            Database::open(encrypted_config(&path, key)).expect("la clave válida sigue abriendo");
        assert_marker(&mut database, &marker);
    }

    /// AC-0105-06 — el rollback cifrado reconstruye los índices en memoria.
    ///
    /// Deja entradas fantasma en vuelo (primario + índice de texto + índice
    /// persistido de la columna, replicando los pasos de `insert_record`) y
    /// verifica que tras `rollback` ninguna estructura conserva la fila
    /// abortada y la base sigue operativa.
    #[test]
    // @spec AC-0105-06
    fn test_ac_0105_06_encrypted_rollback_rebuilds_indexes() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("ac010506.data");
        let mut database = Database::open(encrypted_config(&path, test_key())).expect("open");
        database
            .create_table(
                "t",
                vec![
                    ColumnDef {
                        name: "a".to_string(),
                        col_type: ColumnType::Int,
                    },
                    ColumnDef {
                        name: "b".to_string(),
                        col_type: ColumnType::Text,
                    },
                ],
            )
            .expect("create_table");
        database.create_index("t", "b").expect("indice texto");

        // Fila confirmada (insert con auto-commit): primario=1, FTS=1.
        let mut scalars = ScalarMap::new();
        scalars.insert("a".to_string(), ScalarValue::Int(1));
        scalars.insert("b".to_string(), ScalarValue::Text("confirmado".to_string()));
        database.insert("t", scalars).expect("insert confirmado");
        assert_eq!(database.primary_index_len("t"), 1, "primario confirmado");
        assert_eq!(database.fts_doc_count("t", "b"), 1, "FTS confirmado");

        // Transaccion en vuelo con entradas fantasma (pasos de insert_record:
        // heap -> primario -> indice persistido -> indices en memoria).
        let tx = database.begin();
        let mut catalog = Catalog::load(&mut database).expect("catalogo");
        let definition = catalog.get("t").expect("tabla").clone();
        let mut ghost_scalars = ScalarMap::new();
        ghost_scalars.insert("a".to_string(), ScalarValue::Int(99));
        ghost_scalars.insert("b".to_string(), ScalarValue::Text("fantasma".to_string()));
        let lsn = database.wal.next_lsn();
        let ghost = Record {
            id: RecordId::new(),
            scalars: ghost_scalars,
            doc: None,
            edges: EdgeSet::default(),
            vector: None,
            blob: None,
            meta: RecordMeta {
                created_tx: tx,
                deleted_tx: None,
                lsn,
                embedding_version: None,
            },
        };
        let locator = heap_insert(&mut database, &mut catalog, "t", &ghost).expect("heap fantasma");
        database
            .primary_insert("t", ghost.id, locator)
            .expect("primario fantasma");
        maintain_index(&mut database, &mut catalog, "t", &ghost, locator)
            .expect("indice persistido fantasma");
        let entry = database.indexes.entry("t".to_string()).or_default();
        entry
            .index_record(&ghost, &definition)
            .expect("indices en memoria fantasma");
        entry.finalize();
        assert_eq!(
            database.primary_index_len("t"),
            2,
            "el fantasma entro en el primario"
        );
        assert_eq!(
            database.fts_doc_count("t", "b"),
            2,
            "el fantasma entro en el FTS"
        );

        database.rollback().expect("rollback cifrado");

        assert_eq!(
            database.primary_index_len("t"),
            1,
            "primario reconstruido sin fantasma"
        );
        assert_eq!(
            database.fts_doc_count("t", "b"),
            1,
            "FTS reconstruido sin fantasma"
        );
        assert!(
            database
                .get_record("t", &ghost.id)
                .expect("point lookup del fantasma")
                .is_none(),
            "el id fantasma no debe resolver en el primario"
        );
        assert_eq!(
            database.execute("SELECT * FROM t").expect("select").len(),
            1,
            "solo la fila confirmada es visible"
        );

        // La base sigue operativa tras el rebuild de indices.
        let mut after = ScalarMap::new();
        after.insert("a".to_string(), ScalarValue::Int(3));
        after.insert(
            "b".to_string(),
            ScalarValue::Text("post-rollback".to_string()),
        );
        database.insert("t", after).expect("insert tras rollback");
        assert_eq!(
            database.fts_doc_count("t", "b"),
            2,
            "el nuevo documento se indexa"
        );
        database.close().expect("close");
    }

    proptest! {
        /// Propiedad: filas aleatorias con clave aleatoria sobreviven a reopen.
        #[test]
        fn prop_encrypted_database_roundtrip(
            key in prop::array::uniform32(any::<u8>()),
            values in prop::collection::vec("[a-z]{1,8}", 1..8),
        ) {
            let dir = tempfile::tempdir().expect("dir");
            let path = dir.path().join("prop.data");
            {
                let mut database =
                    Database::open(encrypted_config(&path, key)).expect("open cifrado");
                database
                    .create_table(
                        "t",
                        vec![ColumnDef {
                            name: "v".to_string(),
                            col_type: ColumnType::Text,
                        }],
                    )
                    .expect("create_table");
                for value in &values {
                    let mut scalars = ScalarMap::new();
                    scalars.insert(
                        "v".to_string(),
                        ScalarValue::Text(value.clone()),
                    );
                    database.insert("t", scalars).expect("insert");
                }
                database.commit().expect("commit");
                database.close().expect("close");
            }
            let mut database =
                Database::open(encrypted_config(&path, key)).expect("reopen con clave");
            let rows = database.execute("SELECT * FROM t").expect("select");
            prop_assert_eq!(rows.len(), values.len());
            for (row, expected) in rows.iter().zip(values.iter()) {
                prop_assert_eq!(
                    row.get("v").expect("columna v"),
                    &ScalarValue::Text(expected.clone())
                );
            }
        }
    }

    /// `new` conserva la clave íntegra vía `key()`.
    #[test]
    fn test_encryption_config_new_roundtrip() {
        let config = EncryptionConfig::new(test_key());
        assert_eq!(config.key(), &test_key());
    }

    /// `from_passphrase` es determinista y sensible a sus entradas.
    #[test]
    fn test_encryption_config_from_passphrase() {
        let salt = b"sal-16-bytes!!!!";
        let first = EncryptionConfig::from_passphrase("secreto", salt).expect("derive");
        let second = EncryptionConfig::from_passphrase("secreto", salt).expect("derive");
        assert_eq!(first.key(), second.key());
        let other = EncryptionConfig::from_passphrase("otro", salt).expect("derive");
        assert_ne!(first.key(), other.key());
        assert!(EncryptionConfig::from_passphrase("secreto", b"").is_err());
    }

    /// El `Debug` nunca expone la clave.
    #[test]
    fn test_encryption_config_debug_is_redacted() {
        let config = EncryptionConfig::new([0xABu8; 32]);
        let rendered = format!("{config:?}");
        assert_eq!(rendered, "EncryptionConfig { key: \"[redacted]\" }");
    }
}
