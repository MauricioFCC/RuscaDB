#!/usr/bin/env python3
"""Cliente LLM agnóstico de proveedor para el harness de RuscaDB (SPEC-0100).

Expone un único punto de acceso a un modelo de lenguaje para las tareas de
verificación T2 (LLM-judge, revisión de código multi-agente) sin acoplar el
harness a un backend concreto. La selección se hace por entorno
(``RUSCADB_LLM_PROVIDER``) o por ``--provider``:

* ``local`` (por defecto, **desarrollo**): ``llama.cpp`` ``llama-server`` en
  ``RUSCADB_LLM_BASE_URL`` (por defecto ``http://127.0.0.1:8080/v1``), modelo
  ``RUSCADB_LLM_MODEL`` (por defecto ``qwen3-4b-thinking-2507-q4-k-m``) y una
  API key **opcional** en ``RUSCADB_LLM_API_KEY`` (si falta, **no** se envía
  cabecera ``Authorization``). Nunca exige API key.
* ``openai`` (**producción**): la API oficial de OpenAI en
  ``OPENAI_BASE_URL`` (por defecto ``https://api.openai.com/v1``), con
  ``OPENAI_API_KEY`` **obligatoria** y modelo ``RUSCADB_LLM_MODEL`` (por
  defecto ``gpt-4o-mini``).
* ``ollama`` (compatibilidad **legacy**): cualquier servidor Ollama o
  llama-swap en ``RUSCADB_LLM_BASE_URL`` (por defecto
  ``http://127.0.0.1:11434``), sin API key. Se mantiene para no romper setups
  existentes.

``local`` y ``openai`` comparten el **mismo protocolo** OpenAI-compatible
(``POST {base}/chat/completions``; contenido en ``choices[0].message.content``
y ``Authorization: Bearer`` solo si hay key). ``ollama`` usa el protocolo
nativo (``POST {base}/api/chat`` con ``stream: false``; contenido en
``message.content``). Así hay un solo protocolo moderno y dos configuraciones.

El harness elige el backend por entorno; en producción::

    RUSCADB_LLM_PROVIDER=openai OPENAI_API_KEY=sk-... llm_judge.py change.diff

Uso::

    llm_provider.py --show-config
    llm_provider.py --prompt "resume este diff" --provider openai

Salida: ``0`` éxito; ``2`` error de configuración (falta ``OPENAI_API_KEY``,
proveedor desconocido) o de red/HTTP (fallo ruidoso, nunca verde silencioso).
Solo stdlib de Python (``urllib.request``, ``json``, ``os``, ``sys``).
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request

# Defaults por proveedor (ver `_PROVIDER_SPECS`).
DEFAULT_LOCAL_BASE_URL = "http://127.0.0.1:8080/v1"
DEFAULT_LOCAL_MODEL = "qwen3-4b-thinking-2507-q4-k-m"
DEFAULT_OPENAI_BASE_URL = "https://api.openai.com/v1"
DEFAULT_OPENAI_MODEL = "gpt-4o-mini"
DEFAULT_OLLAMA_BASE_URL = "http://127.0.0.1:11434"
DEFAULT_OLLAMA_MODEL = "phi-4-mini-reasoning-q4-k-m"

PROVIDERS = ("local", "openai", "ollama")
PROTOCOL_OPENAI = "openai"
PROTOCOL_OLLAMA = "ollama"

_ENV_PROVIDER = "RUSCADB_LLM_PROVIDER"
_ENV_MODEL = "RUSCADB_LLM_MODEL"
_ENV_LOCAL_BASE = "RUSCADB_LLM_BASE_URL"
_ENV_LOCAL_KEY = "RUSCADB_LLM_API_KEY"
_ENV_OPENAI_BASE = "OPENAI_BASE_URL"
_ENV_OPENAI_KEY = "OPENAI_API_KEY"
_TIMEOUT_SECONDS = 300

# Especificación data-driven de cada proveedor: protocolo, defaults, variable
# de entorno de la base y de la key, y si la key es obligatoria.
_PROVIDER_SPECS = {
    "local": {
        "protocol": PROTOCOL_OPENAI,
        "default_base": DEFAULT_LOCAL_BASE_URL,
        "default_model": DEFAULT_LOCAL_MODEL,
        "env_base": _ENV_LOCAL_BASE,
        "env_key": _ENV_LOCAL_KEY,
        "key_required": False,
    },
    "openai": {
        "protocol": PROTOCOL_OPENAI,
        "default_base": DEFAULT_OPENAI_BASE_URL,
        "default_model": DEFAULT_OPENAI_MODEL,
        "env_base": _ENV_OPENAI_BASE,
        "env_key": _ENV_OPENAI_KEY,
        "key_required": True,
    },
    "ollama": {
        "protocol": PROTOCOL_OLLAMA,
        "default_base": DEFAULT_OLLAMA_BASE_URL,
        "default_model": DEFAULT_OLLAMA_MODEL,
        "env_base": _ENV_LOCAL_BASE,
        "env_key": None,
        "key_required": False,
    },
}

USAGE = """usage: llm_provider.py (--show-config | --prompt TEXTO)
                      [--provider local|openai|ollama] [--model M] [--base-url URL]
                      [--system TEXTO]

Cliente LLM agnóstico de proveedor para el harness (SPEC-0100).

Proveedores:
  local   (DEFAULT, desarrollo)  llama.cpp `llama-server`; protocolo
          OpenAI-compatible en RUSCADB_LLM_BASE_URL (default
          http://127.0.0.1:8080/v1), modelo RUSCADB_LLM_MODEL (default
          qwen3-4b-thinking-2507-q4-k-m), API key OPCIONAL en
          RUSCADB_LLM_API_KEY (nunca la exige).
  openai  (producción)           API oficial de OpenAI; base OPENAI_BASE_URL
          (default https://api.openai.com/v1), key OPENAI_API_KEY
          (OBLIGATORIA: si falta, exit 2) y modelo RUSCADB_LLM_MODEL (default
          gpt-4o-mini).
  ollama  (legacy, opcional)     protocolo nativo Ollama/llama-swap en
          RUSCADB_LLM_BASE_URL (default http://127.0.0.1:11434), sin key.

Selección por entorno: RUSCADB_LLM_PROVIDER=local|openai|ollama (default
`local`); `--provider` la sobreescribe. En producción el harness usa
RUSCADB_LLM_PROVIDER=openai + OPENAI_API_KEY.

  --show-config   imprime {provider, base_url, model, api_key_present} (sin red).
  --prompt TEXTO  envía el prompt y imprime la respuesta.
  --system TEXTO  mensaje de sistema opcional.
  --model M       sobreescribe RUSCADB_LLM_MODEL.
  --base-url URL  sobreescribe la base del proveedor.

Salida: 0 éxito; 2 error de configuración o de red.
"""


def ensure_utf8_stdio():
    """Fuerza UTF-8 en ``stdout``/``stderr``.

    En Windows la consola usa una code page (p. ej. cp1252) que no puede
    codificar todos los caracteres de un prompt o de un diff; reconfigurar a
    UTF-8 evita ``UnicodeEncodeError`` al imprimir contenido no-ASCII.

    Returns:
        ``None``.
    """
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is None:
            continue
        try:
            reconfigure(encoding="utf-8")
        except (ValueError, OSError):
            # Stream no reconfigurable (redirección inusual): se ignora.
            continue


class LLMConfigError(RuntimeError):
    """Configuración inválida o incompleta (fallo accionable, sin red)."""


class LLMRequestError(RuntimeError):
    """Fallo de red, HTTP o de forma de la respuesta del proveedor."""


class Config:
    """Configuración efectiva resuelta para un proveedor.

    Attributes:
        provider: Proveedor elegido (``local``/``openai``/``ollama``).
        model: Modelo efectivo.
        base_url: URL base efectiva (sin barra final).
        api_key: API key efectiva o ``None`` si no aplica/falta.
        protocol: ``openai`` (OpenAI-compatible) u ``ollama`` (nativo).
        requires_key: ``True`` si el proveedor exige API key.
    """

    __slots__ = ("provider", "model", "base_url", "api_key", "protocol", "requires_key")

    def __init__(self, provider, model, base_url, api_key, protocol, requires_key):
        """Inicializa la configuración efectiva.

        Args:
            provider: Proveedor elegido.
            model: Modelo efectivo.
            base_url: URL base efectiva.
            api_key: API key efectiva o ``None``.
            protocol: Dialecto de protocolo (``openai``/``ollama``).
            requires_key: Si el proveedor exige API key.
        """
        self.provider = provider
        self.model = model
        self.base_url = base_url
        self.api_key = api_key
        self.protocol = protocol
        self.requires_key = requires_key

    def as_dict(self):
        """Serializa la configuración para ``--show-config``.

        Returns:
            Diccionario JSON-serializable con ``provider``, ``base_url``,
            ``model`` y ``api_key_present`` (booleano; nunca la clave).
        """
        return {
            "provider": self.provider,
            "base_url": self.base_url,
            "model": self.model,
            "api_key_present": bool(self.api_key),
        }


def resolve_config(provider=None, model=None, base_url=None):
    """Resuelve la configuración efectiva del proveedor.

    Precedencia por campo: argumento explícito > variable de entorno > default
    del proveedor elegido.

    Args:
        provider: Proveedor forzado (``local``/``openai``/``ollama``); si es
            ``None`` se usa ``RUSCADB_LLM_PROVIDER`` y, en su defecto, ``local``.
        model: Modelo forzado; si es ``None`` se usa ``RUSCADB_LLM_MODEL`` y,
            en su defecto, el default del proveedor.
        base_url: URL base forzada; si es ``None`` se usa la variable de
            entorno del proveedor y, en su defecto, su default.

    Returns:
        Una instancia de ``Config`` con los valores efectivos.

    Raises:
        LLMConfigError: Si el proveedor indicado no está en ``PROVIDERS``.
    """
    chosen = (provider or os.environ.get(_ENV_PROVIDER) or "local").strip().lower()
    if chosen not in _PROVIDER_SPECS:
        raise LLMConfigError(
            f"WHAT: proveedor '{chosen}' desconocido. "
            f"WHY: solo se admite {'/'.join(PROVIDERS)}. "
            f"WHERE: usa --provider {'|'.join(PROVIDERS)} o exporta {_ENV_PROVIDER}."
        )
    spec = _PROVIDER_SPECS[chosen]
    effective_model = model or os.environ.get(_ENV_MODEL) or spec["default_model"]
    effective_base = (
        base_url or os.environ.get(spec["env_base"]) or spec["default_base"]
    ).rstrip("/")
    api_key = None
    if spec["env_key"]:
        api_key = os.environ.get(spec["env_key"]) or None
    return Config(
        chosen,
        effective_model,
        effective_base,
        api_key,
        spec["protocol"],
        spec["key_required"],
    )


def require_credentials(config):
    """Verifica que la configuración tenga credenciales utilizables.

    Args:
        config: Configuración resuelta (``Config``).

    Raises:
        LLMConfigError: Si el proveedor exige API key y esta falta (mensaje
            accionable WHAT/WHY/WHERE, nunca verde silencioso).
    """
    if config.requires_key and not config.api_key:
        raise LLMConfigError(
            f"WHAT: falta {_ENV_OPENAI_KEY} para el proveedor '{config.provider}'. "
            "WHY: la API de OpenAI exige autenticación 'Authorization: Bearer'. "
            f"WHERE: exporta {_ENV_OPENAI_KEY}=sk-... o usa --provider local "
            f"(llama.cpp en {DEFAULT_LOCAL_BASE_URL})."
        )


def _build_request(config, messages):
    """Construye la petición HTTP según el protocolo del proveedor.

    Args:
        config: Configuración efectiva.
        messages: Lista de mensajes ``{"role", "content"}``.

    Returns:
        Tupla ``(url, request)`` lista para ``urllib.request.urlopen``.
    """
    headers = {"Content-Type": "application/json"}
    if config.protocol == PROTOCOL_OLLAMA:
        url = f"{config.base_url}/api/chat"
        payload = {"model": config.model, "messages": messages, "stream": False}
    else:
        url = f"{config.base_url}/chat/completions"
        payload = {"model": config.model, "messages": messages}
        if config.api_key:
            headers["Authorization"] = f"Bearer {config.api_key}"
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=data, headers=headers, method="POST")
    return url, request


def _extract_content(protocol, body, url):
    """Extrae el texto del cuerpo JSON según el dialecto del proveedor.

    Args:
        protocol: Dialecto efectivo (``openai``/``ollama``).
        body: Cuerpo de la respuesta HTTP como texto.
        url: URL invocada (para el mensaje de error).

    Returns:
        El contenido textual del mensaje del modelo.

    Raises:
        LLMRequestError: Si el JSON es inválido o no trae el campo esperado.
    """
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError as error:
        raise LLMRequestError(
            f"WHAT: respuesta no-JSON del proveedor ({url}). "
            f"WHY: {error}. WHERE: revisa el endpoint y el modelo configurados."
        ) from error
    try:
        if protocol == PROTOCOL_OLLAMA:
            content = parsed["message"]["content"]
        else:
            content = parsed["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError) as error:
        expected = (
            "message.content"
            if protocol == PROTOCOL_OLLAMA
            else "choices[0].message.content"
        )
        raise LLMRequestError(
            f"WHAT: la respuesta del proveedor no trae el contenido esperado. "
            f"WHY: falta la ruta '{expected}'. "
            f"WHERE: revisa {url} y el modelo configurado."
        ) from error
    if not isinstance(content, str):
        raise LLMRequestError(
            f"WHAT: el contenido de la respuesta no es texto ({type(content).__name__}). "
            f"WHY: formato inesperado del proveedor. WHERE: revisa {url}."
        )
    return content


def _invoke(config, messages):
    """Envía los mensajes al proveedor y extrae el contenido de la respuesta.

    Args:
        config: Configuración efectiva.
        messages: Lista de mensajes ``{"role", "content"}``.

    Returns:
        El texto de la respuesta del modelo.

    Raises:
        LLMRequestError: Si falla la red, el HTTP o la forma de la respuesta.
    """
    url, request = _build_request(config, messages)
    try:
        with urllib.request.urlopen(request, timeout=_TIMEOUT_SECONDS) as response:
            body = response.read().decode("utf-8")
    except urllib.error.HTTPError as error:
        raise LLMRequestError(
            f"WHAT: el proveedor '{config.provider}' respondió HTTP {error.code}. "
            f"WHY: {error.reason}. "
            f"WHERE: revisa {url}, el modelo '{config.model}' y las credenciales."
        ) from error
    except urllib.error.URLError as error:
        raise LLMRequestError(
            f"WHAT: no se pudo conectar con el proveedor '{config.provider}'. "
            f"WHY: {error.reason}. "
            f"WHERE: verifica que {config.base_url} esté en línea (para 'local', "
            "arranca 'llama-server'; para 'ollama', 'ollama serve')."
        ) from error
    except OSError as error:
        raise LLMRequestError(
            f"WHAT: error de red hablando con '{config.provider}'. "
            f"WHY: {error}. WHERE: revisa la conectividad hacia {config.base_url}."
        ) from error
    return _extract_content(config.protocol, body, url)


def complete(prompt, system=None, *, config=None):
    """Genera una respuesta del LLM para ``prompt``.

    Args:
        prompt: Texto del mensaje de usuario.
        system: Mensaje de sistema opcional (rol/instrucciones).
        config: Configuración efectiva; si es ``None`` se resuelve del entorno.

    Returns:
        El texto de la respuesta del modelo.

    Raises:
        LLMConfigError: Si la configuración es inválida (p. ej. falta la key
            obligatoria del proveedor ``openai``).
        LLMRequestError: Si falla la red, el HTTP o la forma de la respuesta.
    """
    effective = config or resolve_config()
    require_credentials(effective)
    messages = []
    if system:
        messages.append({"role": "system", "content": system})
    messages.append({"role": "user", "content": prompt})
    return _invoke(effective, messages)


def _parse_args(argv):
    """Parsea los argumentos de la CLI de ``llm_provider.py``.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        Diccionario con ``prompt``, ``system``, ``show_config``, ``help``,
        ``provider``, ``model`` y ``base_url``.

    Raises:
        LLMConfigError: Si una opción requiere valor y falta, o si el
            argumento es desconocido.
    """
    options = {
        "prompt": None,
        "system": None,
        "show_config": False,
        "help": False,
        "provider": None,
        "model": None,
        "base_url": None,
    }
    args = list(argv[1:])
    index = 0
    while index < len(args):
        arg = args[index]
        if arg == "--show-config":
            options["show_config"] = True
        elif arg in ("-h", "--help"):
            options["help"] = True
        elif arg in ("--prompt", "--system", "--provider", "--model", "--base-url"):
            if index + 1 >= len(args):
                raise LLMConfigError(
                    f"WHAT: la opción '{arg}' requiere un valor. "
                    f"WHY: se invocó sin argumento. WHERE: añade '{arg} <valor>'."
                )
            options[arg[2:].replace("-", "_")] = args[index + 1]
            index += 1
        else:
            raise LLMConfigError(
                f"WHAT: argumento desconocido '{arg}'. "
                "WHY: no forma parte de la CLI de llm_provider. "
                "WHERE: usa --prompt/--show-config/--provider/--model/--base-url."
            )
        index += 1
    return options


def main(argv):
    """Punto de entrada de la CLI.

    Args:
        argv: Argumentos de línea de comandos (incluye el script en ``[0]``).

    Returns:
        Exit code: ``0`` éxito; ``2`` error de configuración o de red.
    """
    ensure_utf8_stdio()
    try:
        options = _parse_args(argv)
    except LLMConfigError as error:
        print(f"llm-provider: {error}", file=sys.stderr)
        return 2
    if options["help"]:
        print(USAGE)
        return 0
    try:
        config = resolve_config(
            options["provider"], options["model"], options["base_url"]
        )
        if options["show_config"]:
            print(json.dumps(config.as_dict(), indent=2, sort_keys=True))
            require_credentials(config)
            return 0
        if options["prompt"] is None:
            print(USAGE, file=sys.stderr)
            return 2
        answer = complete(options["prompt"], system=options["system"], config=config)
    except LLMConfigError as error:
        print(f"llm-provider: {error}", file=sys.stderr)
        return 2
    except LLMRequestError as error:
        print(f"llm-provider: {error}", file=sys.stderr)
        return 2
    print(answer)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
