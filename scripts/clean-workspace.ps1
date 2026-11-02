# Limpieza de artefactos de compilacion y mutacion (anti-contaminacion).
#
# Uso:  powershell -File scripts/clean-workspace.ps1   (o  pwsh -File ...)
#
# SPEC-0192: delega en `cargo xtask gc --apply --all` (allowlist + reporte) y
# completa con `cargo clean` para el `target/` residual cuando xtask corre
# dentro del propio target (Windows no permite borrar un .exe en uso).
#
# Elimina:
#   - target/ de Cargo (workspace) y crates/*/target
#   - mutants.out / mutants.out.old (cargo-mutants)
#   - coverage/ y perfiles *.profraw/*.profdata/*.lcov
#   - target dirs aislados en %TEMP%\ruscadb-targets
#
# Es idempotente y seguro: no toca codigo fuente, specs, docs ni Cargo.lock.

# "Continue": los comandos nativos (cargo) escriben progreso por stderr y en
# Windows PowerShell 5.1 eso se convierte en ErrorRecord; no debe abortar.
$ErrorActionPreference = "Continue"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    Write-Host "[clean] cargo xtask gc --apply --all"
    cargo xtask gc --apply --all
    if ($LASTEXITCODE -ne 0) { Write-Host "[clean] aviso: gc devolvio $LASTEXITCODE" }

    # Si xtask corrio desde el target del workspace, gc lo omitio para no
    # borrar su propio ejecutable; `cargo clean` lo elimina desde otro proceso.
    Write-Host "[clean] cargo clean (target residual)"
    cargo clean 2>$null
    if ($LASTEXITCODE -ne 0) { Write-Host "[clean] aviso: cargo clean devolvio $LASTEXITCODE" }

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
