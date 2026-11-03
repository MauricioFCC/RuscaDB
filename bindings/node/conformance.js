'use strict';

/**
 * Conformance del fixture Rust desde Node.js (SPEC-0196).
 *
 * Compila la FFI y ejecuta:
 *   cargo build -p ruscadb-ffi
 *   node bindings/node/conformance.js
 */

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const ruscadb = require('./ruscadb');

/**
 * Normaliza números como RFC 8785 para flotantes integrales.
 *
 * @param {number} value Número de entrada.
 * @returns {number|null} Número canónico o `null` si no es finito.
 */
function canonicalNumber(value) {
  // JSON.stringify ya serializa 2.0 como "2" (ECMAScript/JCS); solo
  // normalizamos los no finitos, que el transporte JSON pierde como null.
  return Number.isFinite(value) ? value : null;
}

/**
 * Convierte la forma externa de ScalarValue a JSON plano.
 *
 * @param {unknown} value Valor de entrada.
 * @returns {unknown} Valor normalizado.
 */
function normalize(value) {
  if (value === 'Null') {
    // Variante unitaria externa de ScalarValue::Null (Text("Null") llega
    // como {Text:"Null"} y no se confunde).
    return null;
  }
  if (Array.isArray(value)) {
    return value.map(normalize);
  }
  if (typeof value === 'number') {
    return canonicalNumber(value);
  }
  if (value && typeof value === 'object') {
    const keys = Object.keys(value);
    if (keys.length === 1) {
      const key = keys[0];
      if (['Int', 'UInt', 'TimestampMillis', 'Text', 'Bool', 'Bytes'].includes(key)) {
        return value[key];
      }
      if (key === 'Float') {
        return canonicalNumber(value[key]);
      }
      if (key === 'Null') {
        return null;
      }
    }
    const out = {};
    for (const key of keys.sort()) {
      out[key] = normalize(value[key]);
    }
    return out;
  }
  return value;
}

/**
 * Ordena filas y claves de forma canónica.
 *
 * @param {Array<object>} rows Filas de entrada.
 * @returns {Array<object>} Filas canónicas.
 */
function canonicalRows(rows) {
  return rows
    .map(normalize)
    .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

/**
 * Crea y puebla las tablas `t` y `typed` del fixture.
 *
 * @param {object} handle Handle de RuscaDB.
 */
function seed(handle) {
  ruscadb.createTable(handle, 't', [{ name: 'x', type: 'Int' }]);
  for (const value of [1, 2, 3]) {
    ruscadb.insertRecord(handle, 't', { scalars: { x: { Int: value } } });
  }

  ruscadb.createTable(handle, 'typed', [
    { name: 'f', type: 'Float' },
    { name: 's', type: 'Text' },
    { name: 'b', type: 'Bool' },
    { name: 'n', type: 'Int' },
  ]);
  for (const [f, s, b] of [
    [1.5, 'uno', false],
    [2.0, 'dos', true],
  ]) {
    ruscadb.insertRecord(handle, 'typed', {
      scalars: {
        f: { Float: f },
        s: { Text: s },
        b: { Bool: b },
        n: { Null: null },
      },
    });
  }
}

function main() {
  const fixturePath = path.join(
    __dirname,
    '..',
    '..',
    'crates',
    'ruscadb',
    'tests',
    'conformance',
    'basic.json',
  );
  const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'ruscadb-node-'));
  const dbPath = path.join(tmp, 'conformance.db');
  const hash = crypto.createHash('sha256');
  const handle = ruscadb.open(dbPath, 64);
  try {
    seed(handle);
    for (const [index, testCase] of fixture.cases.entries()) {
      if (testCase.expect_error) {
        let failed = false;
        try {
          ruscadb.execute(handle, testCase.sql);
        } catch (error) {
          failed = true;
        }
        if (!failed) {
          console.error(`FAIL node/${testCase.name}: se esperaba error`);
          process.exitCode = 1;
          return;
        }
        hash.update(testCase.name);
        hash.update('error');
        continue;
      }
      const rows = JSON.parse(ruscadb.execute(handle, testCase.sql));
      const got = canonicalRows(rows);
      if (process.argv.includes('--tamper') && index === 0 && got.length > 0) {
        for (const key of Object.keys(got[0])) {
          if (typeof got[0][key] === 'number') {
            got[0][key] += 1;
            break;
          }
        }
      }
      const expected = canonicalRows(testCase.expect);
      if (JSON.stringify(got) !== JSON.stringify(expected)) {
        console.error(
          `FAIL node/${testCase.name}: got=${JSON.stringify(got)} expected=${JSON.stringify(expected)}`,
        );
        process.exitCode = 1;
        return;
      }
      hash.update(testCase.name);
      hash.update(JSON.stringify(got));
    }
  } finally {
    ruscadb.close(handle);
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  console.log(`node_hash=${hash.digest('hex')}`);
}

main();
