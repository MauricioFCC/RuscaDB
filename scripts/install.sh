#!/usr/bin/env sh
# Instalador del ejecutable `ruscadb` (RuscaDB CLI) para Linux y macOS.
#
# Uso:
#   curl -fsSL https://raw.githubusercontent.com/MauricioFCC/RuscaDB/main/scripts/install.sh | sh
#   RUSCADB_VERSION=v0.1.0 INSTALL_DIR=/usr/local/bin sh scripts/install.sh
#
# Variables de entorno:
#   RUSCADB_VERSION  Version a instalar (por defecto: ultimo release).
#   INSTALL_DIR      Directorio destino (por defecto: ~/.local/bin).
#   RUSCADB_REPO     Owner/repo de GitHub (por defecto: MauricioFCC/RuscaDB).

set -eu

REPO="${RUSCADB_REPO:-MauricioFCC/RuscaDB}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BINARY="ruscadb"

log() {
  printf '[ruscadb] %s\n' "$1"
}

die() {
  printf '[ruscadb] ERROR: %s\n' "$1" >&2
  exit 1
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux) os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) die "sistema no soportado: $os (usa el instalador de Windows: scripts/install.ps1)" ;;
  esac
  case "$arch" in
    x86_64 | amd64) arch_part="x86_64" ;;
    arm64 | aarch64) arch_part="aarch64" ;;
    *) die "arquitectura no soportada: $arch" ;;
  esac
  printf '%s-%s' "$arch_part" "$os_part"
}

resolve_version() {
  if [ -n "${RUSCADB_VERSION:-}" ]; then
    printf '%s' "$RUSCADB_VERSION"
    return
  fi
  version="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$version" ] || die "no se pudo resolver la ultima version"
  printf '%s' "$version"
}

main() {
  target="$(detect_target)"
  version="$(resolve_version)"
  url="https://github.com/${REPO}/releases/download/v${version}/ruscadb-${target}.tar.gz"
  workdir="$(mktemp -d)"
  trap 'rm -rf "$workdir"' EXIT

  log "instalando ruscadb ${version} para ${target}"
  curl -fsSL "$url" -o "${workdir}/ruscadb.tar.gz" || die "fallo la descarga: $url"
  tar -xzf "${workdir}/ruscadb.tar.gz" -C "$workdir"

  if [ -f "${workdir}/ruscadb-${target}.sha256" ]; then
    expected="$(cut -d' ' -f1 < "${workdir}/ruscadb-${target}.sha256")"
    actual="$(sha256sum "${workdir}/ruscadb-${target}" | cut -d' ' -f1)"
    [ "$expected" = "$actual" ] || die "checksum invalido (esperado $expected, obtenido $actual)"
    log "checksum verificado"
  else
    log "aviso: no hay checksum publicado para ${target}"
  fi

  mkdir -p "$INSTALL_DIR"
  install -m 0755 "${workdir}/ruscadb-${target}" "${INSTALL_DIR}/${BINARY}"
  log "instalado en ${INSTALL_DIR}/${BINARY}"

  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *) log "agrega ${INSTALL_DIR} al PATH" ;;
  esac
  log "prueba: ${BINARY} version"
}

main "$@"
