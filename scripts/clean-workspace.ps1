# Limpieza de artefactos de compilacion y mutacion (anti-contaminacion).
#
# Uso:  powershell -File scripts/clean-workspace.ps1   (o  pwsh -File ...)
#
# Elimina:
#   - target/ de Cargo (workspace)
#   - mutants.out / mutants.out.old (cargo-mutants) en cualquier nivel
#   - target dirs aislados en %TEMP%\ruscadb-targets
#
# Es idempotente y seguro: no toca codigo fuente ni Cargo.lock.

# "Continue": los comandos nativos (cargo) escriben progreso por stderr y en
# Windows PowerShell 5.1 eso se convierte en ErrorRecord; no debe abortar.
$ErrorActionPreference = "Continue"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    Write-Host "[clean] cargo clean"
    cargo clean 2>$null
    if ($LASTEXITCODE -ne 0) { Write-Host "[clean] aviso: cargo clean devolvio $LASTEXITCODE" }

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
