<#
.SYNOPSIS
  Instalador del ejecutable `ruscadb` (RuscaDB CLI) para Windows.

.DESCRIPTION
  Descarga el binario oficial del release, verifica el checksum SHA-256 y lo
  instala en el directorio indicado (por defecto %LOCALAPPDATA%\Programs\ruscadb\bin).

.PARAMETER Version
  Version a instalar (por defecto: ultimo release publicado).

.PARAMETER InstallDir
  Directorio destino del ejecutable.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/install.ps1
#>
[CmdletBinding()]
param(
    [string]$Version = "",
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\ruscadb\bin",
    [string]$Repo = "MauricioFCC/RuscaDB"
)

$ErrorActionPreference = "Stop"

function Write-Log {
    param([string]$Message)
    Write-Host "[ruscadb] $Message"
}

function Get-Architecture {
    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -eq "ARM64") { return "aarch64" }
    return "x86_64"
}

function Resolve-Version {
    param([string]$Requested)
    if ($Requested) { return $Requested.TrimStart("v") }
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    if (-not $release.tag_name) { throw "no se pudo resolver la ultima version" }
    return $release.tag_name.TrimStart("v")
}

try {
    $target = "$(Get-Architecture)-pc-windows-msvc"
    $resolved = Resolve-Version -Requested $Version
    $url = "https://github.com/$Repo/releases/download/v$resolved/ruscadb-$target.tar.gz"
    $work = Join-Path ([System.IO.Path]::GetTempPath()) ("ruscadb-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $work | Out-Null

    Write-Log "instalando ruscadb $resolved para $target"
    $archive = Join-Path $work "ruscadb.tar.gz"
    Invoke-WebRequest -Uri $url -OutFile $archive
    tar -xzf $archive -C $work

    $binary = Join-Path $work "ruscadb-$target"
    $checksumFile = Join-Path $work "ruscadb-$target.sha256"
    if (Test-Path $checksumFile) {
        $expected = ((Get-Content $checksumFile) -split "\s+")[0].ToLower()
        $actual = (Get-FileHash -Path $binary -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) {
            throw "checksum invalido (esperado $expected, obtenido $actual)"
        }
        Write-Log "checksum verificado"
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $targetExe = Join-Path $InstallDir "ruscadb.exe"
    Copy-Item -Path $binary -Destination $targetExe -Force
    Write-Log "instalado en $targetExe"

    if (-not ($env:PATH -split ";" -contains $InstallDir)) {
        Write-Log "agrega $InstallDir al PATH (setx PATH `"%PATH%;$InstallDir`")"
    }
    Write-Log "prueba: ruscadb version"
}
finally {
    if ($work -and (Test-Path $work)) { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }
}
