//! # ruscadb-graph
//!
//! Almacén de grafo de RuscaDB: adyacencia **CSR** persistida + delta LSM y
//! traversal BFS/DFS acotado por profundidad y número de nodos.
//!
//! Implementa el puerto `GraphStore` de `ruscadb-core`.
//! Diseño: `docs/RuscaDB-roadmap.md` §5.4. Fase: F3.

#![forbid(unsafe_code)]
