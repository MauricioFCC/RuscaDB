<#
.SYNOPSIS
    Genera el SBOM CycloneDX (JSON) del workspace RuscaDB.

.DESCRIPTION
    F6 - Hardening (SPEC-0011 / docs/RuscaDB-roadmap.md §6.5): produce un
    Software Bill of Materials (SBOM) en formato CycloneDX para el analisis de
    supply chain. No es un gate bloqueante; el job `sbom` de CI lo ejecuta con
    `continue-on-error: true` y sube los `bom.json` como artefacto.

    Ejecuta `cargo cyclonedx --format json --all` desde la raiz del workspace:
    deja un `bom.json` adyacente a cada `Cargo.toml`.

.PARAMETER Install
    Instala `cargo-cyclonedx` (version fijada por el lockfile de cargo) antes
    de generar el SBOM.

.EXAMPLE
    ./scripts/generate_sbom.ps1
    Genera los SBOM usando un cargo-cyclonedx ya instalado.

.EXAMPLE
    ./scripts/generate_sbom.ps1 -Install
    Instala cargo-cyclonedx y luego genera los SBOM.

.NOTES
    Requiere el toolchain Rust del workspace (edition 2024) en el PATH.
#>
[CmdletBinding()]
param(
    [switch]$Install
)

$ErrorActionPreference = "Stop"

# Raiz del workspace (el script vive en scripts/).
$root = Split-Path -Parent $PSScriptRoot

if ($Install) {
    Write-Host "[sbom] instalando cargo-cyclonedx..."
    cargo install cargo-cyclonedx
    if (-not $?) { throw "no se pudo instalar cargo-cyclonedx" }
}

if (-not (Get-Command cargo-cyclonedx -ErrorAction SilentlyContinue)) {
    throw "cargo-cyclonedx no esta instalado. Ejecuta: ./scripts/generate_sbom.ps1 -Install"
}

Push-Location $root
try {
    Write-Host "[sbom] cargo cyclonedx --format json --all"
    cargo cyclonedx --format json --all
    if (-not $?) { throw "cargo cyclonedx fallo" }

    $boms = @(Get-ChildItem -Path $root -Recurse -Filter "bom.json" -File)
    Write-Host "[sbom] OK: $($boms.Count) archivo(s) bom.json generados"
    foreach ($bom in $boms) {
        Write-Host "  - $($bom.FullName)"
    }
}
finally {
    Pop-Location
}
