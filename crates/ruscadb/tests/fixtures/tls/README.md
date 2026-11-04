# Fixtures TLS (solo tests)

Certificado autofirmado de prueba para `localhost` / `127.0.0.1`, válido 10
años, con clave EC P-256. **No es un secreto** y no debe usarse fuera de tests.

- `cert.pem`: certificado X.509 autofirmado (CA=true para poder añadirlo al
  `RootCertStore` del cliente de test).
- `key.pem`: clave privada PKCS#8 sin cifrar.

Generado con `cryptography` (Python) el 2026-11-04 para SPEC-0202.
