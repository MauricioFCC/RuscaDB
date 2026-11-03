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

function normalize(value) {
  if (Array.isArray(value)) {
    return value.map(normalize);
  }
  if (value && typeof value === 'object') {
    const keys = Object.keys(value);
    if (keys.length === 1) {
      const key = keys[0];
      if (
        ['Int', 'UInt', 'Float', 'Text', 'Bool', 'TimestampMillis', 'Bytes'].includes(
          key,
        )
      ) {
        return value[key];
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

function canonicalRows(rows) {
  return rows
    .map(normalize)
    .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
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
    ruscadb.createTable(handle, 't', [{ name: 'x', type: 'Int' }]);
    for (const value of [1, 2, 3]) {
      ruscadb.insertRecord(handle, 't', { scalars: { x: { Int: value } } });
    }
    for (const [index, testCase] of fixture.cases.entries()) {
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
