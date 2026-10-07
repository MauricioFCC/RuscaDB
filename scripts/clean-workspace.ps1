# Limpieza de artefactos de compilacion y mutacion (anti-contaminacion).
#
# Uso:  pwsh scripts/clean-workspace.ps1
#
# Elimina:
#   - target/ de Cargo (workspace)
#   - mutants.out / mutants.out.old (cargo-mutants) en cualquier nivel
#   - target dirs aislados en %TEMP%\ruscadb-targets
#
# Es idempotente y seguro: no toca codigo fuente ni Cargo.lock.

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    Write-Host "[clean] cargo clean"
    cargo clean

    Write-Host "[clean] eliminando mutants.out*"
    Get-ChildItem -Path $root -Recurse -Force -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -in @("mutants.out", "mutants.out.old") } |
        ForEach-Object {
            Write-Host "  - $($_.FullName)"
            Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction SilentlyContinue
        }

    $targets = Join-Path $env:TEMP "ruscadb-targets"
    if (Test-Path -LiteralPath $targets) {
        Write-Host "[clean] eliminando target dirs aislados: $targets"
        Remove-Item -LiteralPath $targets -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "[clean] OK"
}
finally {
    Pop-Location
}
