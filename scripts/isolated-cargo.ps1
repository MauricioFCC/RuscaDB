# Ejecuta cargo con un CARGO_TARGET_DIR unico (aislado) para evitar
# contaminacion entre compilaciones y corridas de mutacion.
#
# Uso:  pwsh scripts/isolated-cargo.ps1 test -p ruscadb
#       pwsh scripts/isolated-cargo.ps1 mutants -p ruscadb-fts
#
# El plugin .opencode/plugin/cargo-isolation.js hace esto automaticamente para
# cada sesion de opencode; este script es el equivalente manual/CI.

param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $CargoArgs
)

$ErrorActionPreference = "Continue"

if (-not $CargoArgs -or $CargoArgs.Count -eq 0) {
    Write-Error "Uso: powershell -File scripts/isolated-cargo.ps1 <args de cargo...>"
    exit 2
}

$id = [Guid]::NewGuid().ToString("N").Substring(0, 8)
$root = if ($env:RUSCADB_TARGET_ROOT) { $env:RUSCADB_TARGET_ROOT } else { Join-Path $env:TEMP "ruscadb-targets" }
$env:CARGO_TARGET_DIR = Join-Path $root "manual-$id"

Write-Host "[isolated-cargo] CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR"
Write-Host "[isolated-cargo] cargo $($CargoArgs -join ' ')"
& cargo @CargoArgs
exit $LASTEXITCODE
